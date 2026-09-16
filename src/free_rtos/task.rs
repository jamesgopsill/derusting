use core::{ffi::CStr, ptr};

use portable_atomic::AtomicPtr;

use crate::free_rtos::bindings::*;

/// A safe wrapper around a FreeRTOS task.
pub struct Task {
    #[allow(unused)]
    inner: *mut RtosTask,
}

impl Task {
    /// Create a new Task that is given a name, stack_depth and the function that it
    /// should run.
    #[allow(unused)]
    pub fn new(
        name: &CStr,
        stack_depth: u16,
        priority: u32,
        fcn: TaskFn,
    ) -> Result<Self, FreeRtosError> {
        let ptr: *mut RtosTask = ptr::null_mut();
        let res = unsafe {
            xTaskCreate(
                fcn,
                name.as_ptr(),
                stack_depth,
                ptr::null_mut(),
                priority,
                ptr,
            )
        };
        match res {
            FreeRtosError::Ok => Ok(Self { inner: ptr }),
            _ => Err(res),
        }
    }

    pub fn new_static(
        name: &CStr,
        fcn: TaskFn,
        priority: u32,
        stack_buf: &mut [u8],
        tcb_buf: &mut [u8],
    ) -> Self {
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

    #[allow(unused)]
    pub fn as_mut_ptr(&self) -> *mut RtosTask {
        self.inner
    }
}

/// Turn a mutable pointer to a FreeRTOS task into a safe Task.
impl TryFrom<*mut RtosTask> for Task {
    type Error = ();

    fn try_from(value: *mut RtosTask) -> Result<Self, Self::Error> {
        if value.is_null() {
            Err(())
        } else {
            Ok(Task { inner: value })
        }
    }
}

impl TryFrom<&AtomicPtr<RtosTask>> for Task {
    type Error = ();

    fn try_from(value: &AtomicPtr<RtosTask>) -> Result<Self, Self::Error> {
        let value = value.load(core::sync::atomic::Ordering::SeqCst);
        Self::try_from(value)
    }
}
