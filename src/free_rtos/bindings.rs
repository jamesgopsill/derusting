#![allow(unused, non_camel_case_types)]
use core::{
    ffi::{c_char, c_long, c_void},
    marker::{PhantomData, PhantomPinned},
};

/// FreeRTOS's `BaseType_t` (a signed 32-bit integer on this port).
pub type BaseType_t = i32;
/// Opaque pointer type for the argument passed to a task's entry function.
pub type pvParameters = c_void;
/// The entry-point signature FreeRTOS expects for a task function.
/// Task functions must never return, and are only valid to call from the
/// FreeRTOS task they were created for.
pub type TaskFunction_t = unsafe extern "C" fn(*mut pvParameters) -> !;
/// Opaque FreeRTOS task control block; only ever used behind a pointer.
#[repr(C)]
pub struct tskTaskControlBlock {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}
/// Handle to a FreeRTOS task (pointer to its control block).
pub type TaskHandle_t = *mut tskTaskControlBlock;

/// Error codes returned by our FreeRTOS task-creation bindings.
#[derive(Debug)]
pub struct Error(i32);

impl core::fmt::Display for Error {
    /// Formats the error using its `Debug` representation (the raw code).
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl core::error::Error for Error {}

impl Error {
    /// Wraps a raw FreeRTOS return code.
    pub fn new(code: i32) -> Self {
        Self(code)
    }

    /// Converts a FreeRTOS return code into a `Result`: `1` (`pdPASS`) is
    /// `Ok`, anything else is an `Error` carrying that code.
    pub fn check(rc: i32) -> Result<(), Self> {
        if (rc == 1) { Ok(()) } else { Err(Self(rc)) }
    }
}

// SAFETY: (applies to the whole block) these are raw FreeRTOS kernel entry points. Unless
// individually noted otherwise, all task-handle arguments must either be
// null (meaning "the calling task", where the API supports it) or a handle
// obtained from FreeRTOS itself (e.g. `xTaskGetCurrentTaskHandle`) and not
// yet deleted; pointer/buffer arguments must be valid for the sizes passed;
// and functions that are only safe from task vs. ISR context are documented
// per-function below.
unsafe extern "C" {
    /// Delete a FreeRTOS task.
    pub fn vTaskDelete(task: TaskHandle_t);

    /// Notify a task to make progress outside of an interrupt.
    pub fn xTaskGenericNotify(
        task: TaskHandle_t,
        index: u32, // index to notify (usually 0)
        ul_value: u32,
        action: u32, // eNotifyAction (2 = eIncrement)
        previous_notification: *mut u32,
    ) -> BaseType_t;

    /// Notify a task to make progress when in an interrupt context.
    /// Must only be called from within an ISR (use `xTaskGenericNotify`
    /// otherwise); `pxHigherPriorityTaskWoken` must point to a valid,
    /// writable `i32`.
    pub fn xTaskGenericNotifyFromISR(
        task: TaskHandle_t,
        index: u32,
        ul_value: u32,
        action: u32,
        previous_notification: *mut u32,
        pxHigherPriorityTaskWoken: *mut i32,
    ) -> BaseType_t;

    /// Wait a specified time or notification to make progress.
    pub fn ulTaskGenericNotifyTake(
        ux_index_to_wait_on: usize,
        x_clear_count_on_exit: i32,
        xTicksToWait: u32,
    ) -> u32;

    /// Get a pointer to the current task.
    pub fn xTaskGetCurrentTaskHandle() -> TaskHandle_t;

    /// Create a FreeRTOS task using caller-provided stack and TCB storage.
    ///
    /// `stack_buf_ptr` must point to a buffer of at least
    /// `us_stack_depth * 4` bytes, and `tcb_buf_ptr` to a buffer sized for
    /// FreeRTOS's internal TCB struct (128 bytes is assumed by callers in
    /// this crate). Both buffers must remain valid and must not be accessed
    /// by anything else for as long as the created task exists, since
    /// FreeRTOS takes ownership of them for the task's lifetime rather than
    /// copying them.
    pub fn xTaskCreateStatic(
        // Pointer to your extern "C" Rust function
        px_task_code: TaskFunction_t,
        // Name of the task
        pc_name: *const c_char,
        // Stack depth in words
        us_stack_depth: u16,
        // Arguments to be passed to the task
        pv_parameters: *mut pvParameters,
        // Task Priority
        ux_priority: u32,
        // Stack buffer
        stack_buf_ptr: *mut u8,
        // TCB buffer ~[0u8; 128]
        tcb_buf_ptr: *mut u8,
    ) -> TaskHandle_t;

    /// Delay a task.
    pub fn vTaskDelay(ticks: u32);

    /// Alloc some FreeRTOS managed heap memory.
    pub fn pvPortMalloc(size: usize) -> *mut c_void;

    /// Free some FreeRTOS managed heap memory.
    pub fn vPortFree(ptr: *mut u8);

    /// Get the current time in ticks.
    pub fn xTaskGetTickCount() -> u32;

    /// Enter a critical section.
    pub fn vPortEnterCritical();

    /// Exit a critical section.
    pub fn vPortExitCritical();

    /// Free heap size.
    pub fn xPortGetFreeHeapSize() -> usize;

    /// Minimum free stack space (in words) that has remained for the task
    /// since it started; `xTask` must be a valid handle (or null for the
    /// calling task).
    pub fn uxTaskGetStackHighWaterMark(xTask: TaskHandle_t) -> usize;
}
