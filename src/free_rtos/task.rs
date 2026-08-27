use core::{ffi::CStr, ptr};

use portable_atomic::AtomicPtr;

use crate::free_rtos::bindings::*;

/// A safe wrapper around a FreeRTOS task.
pub struct Task {
    inner: *mut RtosTask,
}

impl Task {
    /// Create a new Task that is given a name, stack_depth and the function that it
    /// should run.
    pub fn new(name: &CStr, stack_depth: u16, fcn: TaskFn) -> Result<Self, FreeRtosError> {
        let ptr: *mut RtosTask = ptr::null_mut();
        let res = unsafe {
            xTaskCreate(
                fcn,
                name.as_ptr(),
                stack_depth,
                ptr::null_mut(),
                2, // Low priority
                ptr,
            )
        };
        match res {
            FreeRtosError::Ok => Ok(Self { inner: ptr }),
            _ => Err(res),
        }
    }

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
