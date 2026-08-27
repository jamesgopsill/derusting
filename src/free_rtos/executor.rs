use core::{ffi::c_void, marker::PhantomData, ptr};

use embassy_executor::{Spawner, raw};

use crate::{
    DRIVER,
    free_rtos::bindings::{RtosTask, xTaskGenericNotify, xTaskGenericNotifyFromISR},
    log_info,
};

/// An embassy executor designed to be run within a FreeRTOS task.
/// Wraps around a raw Embassy Executor.
pub struct FreeRtosTaskExecutor {
    inner: raw::Executor,
    not_send: PhantomData<*mut ()>,
}

impl FreeRtosTaskExecutor {
    /// Create a new instance of the Executor
    pub fn new(task: *mut RtosTask) -> Self {
        Self {
            inner: raw::Executor::new(task as _),
            not_send: PhantomData,
        }
    }

    /// Provide the executor with the spawn function and run the
    /// executor.
    pub fn run(&'static mut self, init: impl FnOnce(Spawner)) -> ! {
        log_info!("Spawning Tasks");
        init(self.inner.spawner());

        loop {
            unsafe { self.inner.poll() };
            DRIVER.wait_for_interrupt_or_timeout();
        }
    }
}

/// The pender is the function that is used to wake the FreeRTOS
/// task that the executor resides within.
#[unsafe(export_name = "__pender")]
pub fn __pender(context: *mut c_void) {
    // Pender fires when a embassy-sync (Signal/Channel) gets fired.
    // This is then used in turn to wake up the executor to poll again.
    if context.is_null() {
        return;
    }
    let task_handle = context as *mut RtosTask;

    // Determine if we're in an interrupt or non-interrupt state.
    let ipsr: u32;
    unsafe {
        core::arch::asm!(
            "mrs {0}, ipsr",
            out(reg) ipsr,
            options(nomem, nostack, preserves_flags)
        );
    }
    if ipsr != 0 {
        let mut higher_priority_task_woken = 0;
        unsafe {
            xTaskGenericNotifyFromISR(
                task_handle,
                0,
                0,
                2,
                ptr::null_mut(),
                &mut higher_priority_task_woken,
            );
            // On STM32 (Cortex-M), the standard FreeRTOS port triggers a context
            // switch by pending the PendSV interrupt.
            if higher_priority_task_woken != 0 {
                // This is equivalent to portYIELD_FROM_ISR
                cortex_m::peripheral::SCB::set_pendsv();
            }
        }
    } else {
        unsafe {
            xTaskGenericNotify(task_handle, 0, 0, 2, ptr::null_mut());
        }
    }
}
