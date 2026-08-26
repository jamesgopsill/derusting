#![no_std]

use core::{cell::RefCell, ptr};

use critical_section::Mutex;
use embassy_time_queue_utils::Queue;
use portable_atomic::{AtomicPtr, AtomicU32, AtomicU64};
use static_cell::StaticCell;

use crate::{
    free_rtos::{
        alloc::FreeRtosAllocator,
        bindings::{RtosTask, RtosTaskParams, xTaskGetCurrentTaskHandle},
        executor::FreeRtosTaskExecutor,
        task::Task,
        time_driver::FreeRtosTimeDriver,
    },
    tasks::heartbeat,
};

extern crate alloc;

mod free_rtos;
mod log;
mod panic;
mod tasks;

#[global_allocator]
static ALLOCATOR: FreeRtosAllocator = FreeRtosAllocator;

// Instantiate our Embassy Time Driver the interacts with FreeRTOS.
// Designed for Embassy executors running inside a FreeRTOS task.
embassy_time_driver::time_driver_impl!(static DRIVER: FreeRtosTimeDriver = FreeRtosTimeDriver {
    queue: Mutex::new(RefCell::new(Queue::new())),
    timekeeper: AtomicU64::new(u64::MIN),
    free_rtos_now: AtomicU32::new(u32::MIN),

});

/// Keep track of our FreeRTOS task that is running our Embassy executor.
static TASK: AtomicPtr<RtosTask> = AtomicPtr::new(ptr::null_mut());

/// Static store for our Embassy Executor.
static EXECUTOR: StaticCell<FreeRtosTaskExecutor> = StaticCell::new();

/// # Safety
/// We will ensure that we call this function in an
/// appropriate place in the Buddy firmware.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn derusting_main() {
    log_info!("derusting_main()");
    match Task::try_from(&TASK) {
        Ok(_) => log_info!("Embassy has been created"),
        // No task (i.e., null ptr) so create it
        Err(_) => match Task::new(c"Embassy", 2048, embassy) {
            Ok(t) => {
                log_info!("Embassy task created.");
                TASK.store(t.as_mut_ptr(), core::sync::atomic::Ordering::SeqCst);
            }
            Err(_) => log_error!("Failed to create Embassy task."),
        },
    }
}

/// Our FreeRTOS Embassy Task that spawns and never returns.
#[unsafe(no_mangle)]
unsafe extern "C" fn embassy(_pv_parameters: *mut RtosTaskParams) -> ! {
    log_info!("Rust Embassy Task");

    let current_task = unsafe { xTaskGetCurrentTaskHandle() };
    if current_task.is_null() {
        log_error!("We should only be called within a FreeRTOS task.");
    }

    log_info!("Initialising Executor");
    let executor = EXECUTOR.init(FreeRtosTaskExecutor::new(current_task as _));

    executor.run(|spawner| match heartbeat() {
        Ok(t) => spawner.spawn(t),
        Err(e) => log_error!("Spawn Error: {e}"),
    })
}
