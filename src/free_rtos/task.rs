use core::{ffi::CStr, ptr, sync::atomic::AtomicPtr};

use crate::free_rtos::bindings::*;

/// A safe wrapper around a FreeRTOS task.
pub struct Task {
    #[allow(unused)]
    inner: *mut BaseType_t,
}

impl Task {
    /// Create a new Task that is given a name, stack_depth and the function that it
    /// should run.
    #[allow(unused)]
    pub fn new(
        name: &CStr,
        stack_depth: u16,
        priority: u32,
        fcn: TaskFunction_t,
    ) -> Result<Self, FreeRtosError> {
        let mut ptr: *mut BaseType_t = ptr::null_mut();
        // SAFETY: `name` is a valid, nul-terminated C string for the
        // duration of the call; `&mut ptr` is a valid, writable out-pointer
        // we own; `fcn` is a proper `extern "C" fn(*mut pvParameters) -> !`
        // task entry point as required by `TaskFunction_t`.
        let res = unsafe {
            xTaskCreate(
                fcn,
                name.as_ptr(),
                stack_depth,
                ptr::null_mut(),
                priority,
                &mut ptr,
            )
        };
        match res {
            FreeRtosError::Ok => Ok(Self { inner: ptr }),
            _ => Err(res),
        }
    }

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
    ) -> Self {
        // SAFETY: `name` is a valid, nul-terminated C string for the call;
        // `stack_buf`/`tcb_buf` are sized and kept alive per the contract
        // documented above; `fcn` is a valid task entry point.
        let task = unsafe {
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
        Self { inner: task }
    }

    /// Returns the raw FreeRTOS task handle this `Task` wraps.
    #[allow(unused)]
    pub fn as_mut_ptr(&self) -> *mut BaseType_t {
        self.inner
    }
}

/// Turn a mutable pointer to a FreeRTOS task into a safe Task.
impl TryFrom<*mut BaseType_t> for Task {
    type Error = ();

    fn try_from(value: *mut BaseType_t) -> Result<Self, Self::Error> {
        if value.is_null() {
            Err(())
        } else {
            Ok(Task { inner: value })
        }
    }
}

/// Turn an atomically-stored FreeRTOS task handle into a safe `Task`.
impl TryFrom<&AtomicPtr<BaseType_t>> for Task {
    type Error = ();

    fn try_from(value: &AtomicPtr<BaseType_t>) -> Result<Self, Self::Error> {
        let value = value.load(core::sync::atomic::Ordering::SeqCst);
        Self::try_from(value)
    }
}
