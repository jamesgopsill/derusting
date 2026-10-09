use core::{
    ffi::CStr,
    ptr::{self, NonNull},
};

use crate::free_rtos::bindings::*;

/// A safe wrapper around a FreeRTOS task.
#[allow(unused)]
pub struct Task(NonNull<tskTaskControlBlock>);

impl Task {
    /// # Safety (caller contract, not marked `unsafe fn` but relied upon)
    /// `stack_buf` and `tcb_buf` must be `'static` (or otherwise outlive the
    /// created task) and must not be read or written by anything else once
    /// passed in, since FreeRTOS uses them in place as the task's stack and
    /// TCB for as long as the task exists.
    pub fn new_static(
        name: &CStr,
        fcn: TaskFunction_t,
        priority: u32,
        stack_buf: &mut [u8],
        tcb_buf: &mut [u8],
    ) -> Option<Self> {
        // SAFETY: `name` is a valid, nul-terminated C string for the call;
        // `stack_buf`/`tcb_buf` are sized and kept alive per the contract
        // documented above; `fcn` is a valid task entry point.
        let handle = unsafe {
            xTaskCreateStatic(
                fcn,
                name.as_ptr(),
                (stack_buf.len() / 4) as u16,
                ptr::null_mut(),
                priority,
                stack_buf.as_mut_ptr(),
                tcb_buf.as_mut_ptr(),
            )
        };
        Self::from_raw(handle)
    }

    pub fn from_raw(handle: TaskHandle_t) -> Option<Self> {
        NonNull::new(handle).map(Self)
    }
}
