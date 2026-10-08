#![no_std]
#![allow(unused)]

use core::{
    cell::{Cell, RefCell},
    default,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    ptr::read,
};

use critical_section::Mutex as CsMutex;
use embassy_executor::Spawner;
use embassy_futures::{
    join::join,
    select::{self, select},
};
use embassy_sync::{
    blocking_mutex::raw::ThreadModeRawMutex,
    zerocopy_channel::{Channel, Receiver, Sender},
};
use embassy_time::{Duration, Ticker, Timer};
use embassy_time_queue_utils::Queue;
use service::{TcpListener as _, TcpStream as _, UdpSocket as _};
use static_cell::{ConstStaticCell, StaticCell};

use crate::{
    free_rtos::{
        bindings::{pvParameters, vTaskDelay, xTaskGetCurrentTaskHandle},
        executor::FreeRtosTaskExecutor,
        task::Task,
        time_driver::FreeRtosTimeDriver,
    },
    lwip::UdpSocket,
    service::{TcpListener as _, UdpSocket as _, Vfs},
};

mod free_rtos;
mod log;
mod lwip;
mod panic;
mod platform;
mod rng;
mod service;
mod vfs;

// Instantiate our Embassy Time Driver the interacts with FreeRTOS.
// Designed for Embassy executors running inside a FreeRTOS task.
embassy_time_driver::time_driver_impl!(static DRIVER: FreeRtosTimeDriver = FreeRtosTimeDriver {
    queue: CsMutex::new(RefCell::new(Queue::new())),
    timekeeper: CsMutex::new(Cell::new(u64::MIN)),
    free_rtos_now: CsMutex::new(Cell::new(u32::MIN)),
});

/// Static store for our Embassy Executor.
static EXECUTOR: StaticCell<FreeRtosTaskExecutor> = StaticCell::new();

/// Reserving space for our task at compile time.
/// We only need a small stack to hold the executor. The
/// embassy task macro provides the stack memory required
/// for each embassy task
const STACK_BYTES: usize = 1024 * 4; // / 4 for u32 stack words
// #[unsafe(link_section = ".ccmram")]
static mut RTOS_STACK: [u8; STACK_BYTES] = [0u8; STACK_BYTES];
// #[unsafe(link_section = ".ccmram")]
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

    log_info!("Initialising Executor");
    let executor = EXECUTOR.init(FreeRtosTaskExecutor::new(current_task as _));

    executor.run(|spawner| match embassy_main(spawner) {
        Ok(t) => spawner.spawn(t),
        Err(e) => log_error!("Spawn Error: {e}"),
    })
}

/// The main Embassy task: brings up UDP/TCP, waits for an IP address, then
/// spawns the heartbeat, address-book, ledger and TCP worker tasks.
#[embassy_executor::task(pool_size = 1)]
async fn embassy_main(_spawner: Spawner) {
    log_info!("Embassy Main");

    let empty = Ipv4Addr::new(0, 0, 0, 0);
    loop {
        let Some(local) = lwip::local_ipv4() else {
            Timer::after_millis(500).await;
            continue;
        };
        if local == empty {
            Timer::after_millis(500).await;
            continue;
        }
        log_info!("Network IP: {local:?}");
        break;
    }
    let platform = platform::Platform::default();

    let Ok(mut service) =
        service::Service::<lwip::UdpSocket, lwip::TcpListener, _, vfs::File>::new(platform).await
    else {
        log_error!("Failed to create service.");
        return;
    };
    service.run().await;
}
