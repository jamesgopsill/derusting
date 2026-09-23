#![no_std]

use core::cell::RefCell;

use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::Mutex, mutex::Mutex as AsyncMutex};
use embassy_time::Timer;
use embassy_time_queue_utils::Queue;
use heapless::LinearMap;
use portable_atomic::{AtomicU32, AtomicU64};
use static_cell::StaticCell;

use crate::{
    free_rtos::{
        alloc::FreeRtosAllocator,
        bindings::{
            pvParameters, uxTaskGetStackHighWaterMark, vTaskDelay, xPortGetFreeHeapSize,
            xTaskGetCurrentTaskHandle,
        },
        executor::FreeRtosTaskExecutor,
        task::Task,
        time_driver::FreeRtosTimeDriver,
    },
    kinds::{AddressBook, JobLedger},
    lwip::{my_ipaddr, tcp::TcpListener, udp::UdpSocket},
    marlin::{is_ready, set_offline},
    tasks::{
        tcp::tcp_worker,
        udp::{address_book_lifetime_check, heartbeat, manage_ledger, udp_receiver},
    },
};

extern crate alloc;

mod free_rtos;
mod fs;
mod http;
mod kinds;
mod log;
mod lwip;
mod marlin;
mod panic;
mod tasks;

pub const UDP_PORT: u16 = 9090;
pub const TCP_PORT: u16 = 8080;

/// Define our global allocator for those times we want to make use
/// of `alloc` and the heap.
#[global_allocator]
static ALLOCATOR: FreeRtosAllocator = FreeRtosAllocator;

// Instantiate our Embassy Time Driver the interacts with FreeRTOS.
// Designed for Embassy executors running inside a FreeRTOS task.
embassy_time_driver::time_driver_impl!(static DRIVER: FreeRtosTimeDriver = FreeRtosTimeDriver {
    queue: Mutex::new(RefCell::new(Queue::new())),
    timekeeper: AtomicU64::new(u64::MIN),
    free_rtos_now: AtomicU32::new(u32::MIN),
});

/// Static store for our Embassy Executor.
static EXECUTOR: StaticCell<FreeRtosTaskExecutor> = StaticCell::new();

/// Reserving space for our task at compile time.
const STACK_BYTES: usize = 1024 * 10; // / 4 for u32 stack words
static mut RTOS_STACK: [u8; STACK_BYTES] = [0u8; STACK_BYTES];
static mut RTOS_TCB: [u8; 128] = [0u8; 128];

/// # Safety
/// We will ensure that we call this function in an
/// appropriate place in the Buddy firmware. Additionally, this must be called at
/// most once for the life of the program: `RTOS_STACK`/`RTOS_TCB` are handed
/// to FreeRTOS as the new task's stack/TCB storage and remain
/// aliased by that task for as long as it exists, so a second call would
/// create two tasks sharing (and
/// corrupting) the same underlying memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn derusting_main() {
    log_info!("derusting_main()");
    // SAFETY: not yet borrowed elsewhere and, per the fn-level Safety note,
    // this function runs at most once, so this is the only live reference
    // to `RTOS_STACK`.
    #[allow(static_mut_refs)]
    let stack = unsafe { RTOS_STACK.as_mut_slice() };
    // SAFETY: same reasoning as `RTOS_STACK` above, for `RTOS_TCB`.
    #[allow(static_mut_refs)]
    let tcb = unsafe { RTOS_TCB.as_mut_slice() };
    let _ = Task::new_static(c"Embassy", embassy, 1, stack, tcb);
}

/// Our FreeRTOS Embassy Task that spawns and never returns.
///
/// # Safety
/// Must only be invoked by FreeRTOS as the task entry point installed via
/// `Task::new_static` in `derusting_main`; it assumes it is running with a
/// valid FreeRTOS task context (so `vTaskDelay`/`xTaskGetCurrentTaskHandle`
/// below are well-defined) and that `RTOS_STACK`/`RTOS_TCB` remain alive and
/// exclusively owned by this task for as long as it runs.
#[unsafe(no_mangle)]
unsafe extern "C" fn embassy(_pv_parameters: *mut pvParameters) -> ! {
    log_info!("Rust Embassy Task. Waiting 5secs...");
    // SAFETY: called from within a live FreeRTOS task context (see fn-level
    // Safety note); `vTaskDelay` has no pointer/lifetime preconditions.
    unsafe {
        vTaskDelay(5 * 1_000);
    }
    log_info!("Rust Waking up...");

    // SAFETY: `xTaskGetCurrentTaskHandle` is safe to call from any FreeRTOS
    // task context and returns a handle we only use for a null check here.
    let current_task = unsafe { xTaskGetCurrentTaskHandle() };
    if current_task.is_null() {
        log_error!("We should only be called within a FreeRTOS task.");
    }

    match fs::stat(c"/usb/firmware.bbf") {
        Ok(info) => log_info!("bbf file_size: {}", info.fsize),
        Err(e) => log_error!("stat error: {e}"),
    }

    log_info!("Is Ready: {}", is_ready());
    set_offline();

    // TODO: Clear `.gcode` files from the USB stick if it has old jobs on it.
    log_info!("Initialising Executor");

    let executor = EXECUTOR.init(FreeRtosTaskExecutor::new(current_task as _));

    executor.run(|spawner| match embassy_main(spawner) {
        Ok(t) => spawner.spawn(t),
        Err(e) => log_error!("Spawn Error: {e}"),
    })
}

pub const ADDRESS_BOOK_ENTRIES: usize = 32;
pub const UDP_CHANNEL_SIZE: usize = 12;
pub const MAX_TCP_CONNECTIONS: usize = 2;
pub const MAX_TCP_CONNECTION_CHANNEL_SIZE: usize = 12;

static ADDRESS_BOOK: AddressBook<ADDRESS_BOOK_ENTRIES> = AsyncMutex::new(LinearMap::new());
static JOB_LEDGER: JobLedger = AsyncMutex::new(None);
static UDP: StaticCell<UdpSocket<UDP_CHANNEL_SIZE>> = StaticCell::new();
static TCP: StaticCell<TcpListener<MAX_TCP_CONNECTIONS, MAX_TCP_CONNECTION_CHANNEL_SIZE>> =
    StaticCell::new();

/// The main Embassy task: brings up UDP/TCP, waits for an IP address, then
/// spawns the heartbeat, address-book, ledger and TCP worker tasks.
#[embassy_executor::task(pool_size = 1)]
async fn embassy_main(spawner: Spawner) {
    log_stack_and_heap_size();
    let address_book = &ADDRESS_BOOK;
    let ledger = &JOB_LEDGER;

    let udp = UDP.init_with(UdpSocket::<UDP_CHANNEL_SIZE>::new);
    if udp.bind(UDP_PORT).await.is_err() {
        log_critical!("UDP failed");
        return;
    };
    log_info!("UDP up on {UDP_PORT}");

    let tcp =
        TCP.init_with(TcpListener::<MAX_TCP_CONNECTIONS, MAX_TCP_CONNECTION_CHANNEL_SIZE>::new);
    if let Err(err) = tcp.listen(TCP_PORT).await {
        log_critical!("TCP Failed: {err:?}");
        return;
    };
    log_info!("TCP up on {TCP_PORT}");

    // Wait for an IP address
    loop {
        if my_ipaddr().is_some() {
            break;
        };
        Timer::after_secs(1).await;
    }

    match heartbeat(udp) {
        Ok(t) => spawner.spawn(t),
        Err(e) => log_error!("Spawn Error: {e}"),
    }
    match address_book_lifetime_check(address_book) {
        Ok(t) => spawner.spawn(t),
        Err(e) => log_error!("Spawn Error: {e}"),
    }
    match manage_ledger(udp, address_book, ledger) {
        Ok(t) => spawner.spawn(t),
        Err(e) => log_error!("Spawn Error: {e}"),
    }
    match udp_receiver(udp, address_book, ledger) {
        Ok(t) => spawner.spawn(t),
        Err(e) => log_error!("Spawn Error: {e}"),
    }
    for _i in 0..1 {
        match tcp_worker(tcp, udp, address_book, ledger) {
            Ok(t) => spawner.spawn(t),
            Err(e) => log_error!("Spawn Error: {e}"),
        }
    }
}

/// Logs the current task's free stack headroom and the FreeRTOS heap's
/// free byte count, for diagnosing stack/heap pressure.
fn log_stack_and_heap_size() {
    // SAFETY: `uxTaskGetStackHighWaterMark` accepts a null handle to mean
    // "the calling task"; this function is only ever called from inside the
    // Embassy/FreeRTOS task, so that's the task we intend to query.
    let free_stack_words = unsafe { uxTaskGetStackHighWaterMark(core::ptr::null_mut()) };
    let free_stack_bytes = free_stack_words * core::mem::size_of::<usize>(); // 4 bytes on 32-bit ARM

    // SAFETY: `xPortGetFreeHeapSize` takes no pointer arguments and has no
    // preconditions beyond the FreeRTOS heap being initialised, which it is
    // by the time any task (including this one) is running.
    log_info!("Stack: {} bytes, Heap: {}", free_stack_bytes, unsafe {
        xPortGetFreeHeapSize()
    });
}
