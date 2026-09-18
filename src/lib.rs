#![no_std]

use core::cell::RefCell;

//use critical_section::Mutex;
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
            RtosTaskParams, uxTaskGetStackHighWaterMark, vTaskDelay, xPortGetFreeHeapSize,
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
/// appropriate place in the Buddy firmware.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn derusting_main() {
    log_info!("derusting_main()");
    #[allow(static_mut_refs)]
    let stack = unsafe { RTOS_STACK.as_mut_slice() };
    #[allow(static_mut_refs)]
    let tcb = unsafe { RTOS_TCB.as_mut_slice() };
    let _ = Task::new_static(c"Embassy", embassy, 1, stack, tcb);
}

/// Our FreeRTOS Embassy Task that spawns and never returns.
#[unsafe(no_mangle)]
unsafe extern "C" fn embassy(_pv_parameters: *mut RtosTaskParams) -> ! {
    log_info!("Rust Embassy Task. Waiting 5secs...");
    unsafe {
        vTaskDelay(5 * 1_000);
    }
    log_info!("Rust Waking up...");

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
pub const MAX_TCP_CONNECTIONS: usize = 1;
pub const MAX_TCP_CONNECTION_CHANNEL_SIZE: usize = 12;

static ADDRESS_BOOK: AddressBook<ADDRESS_BOOK_ENTRIES> = AsyncMutex::new(LinearMap::new());
static JOB_LEDGER: JobLedger = AsyncMutex::new(None);
static UDP: StaticCell<UdpSocket<UDP_CHANNEL_SIZE>> = StaticCell::new();
static TCP: StaticCell<TcpListener<MAX_TCP_CONNECTIONS, MAX_TCP_CONNECTION_CHANNEL_SIZE>> =
    StaticCell::new();

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
    for _i in 0..2 {
        match tcp_worker(tcp, udp, address_book, ledger) {
            Ok(t) => spawner.spawn(t),
            Err(e) => log_error!("Spawn Error: {e}"),
        }
    }
}

fn log_stack_and_heap_size() {
    let free_stack_words = unsafe { uxTaskGetStackHighWaterMark(core::ptr::null_mut()) };
    let free_stack_bytes = free_stack_words * core::mem::size_of::<usize>(); // 4 bytes on 32-bit ARM

    log_info!("Stack: {} bytes, Heap: {}", free_stack_bytes, unsafe {
        xPortGetFreeHeapSize()
    });
}
