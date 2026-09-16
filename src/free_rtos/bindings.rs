#![allow(unused)]
use core::ffi::{c_char, c_void};

pub type RtosTask = c_void;
pub type RtosTaskParams = c_void;
pub type TaskFn = unsafe extern "C" fn(*mut RtosTaskParams) -> !;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[repr(i32)]
pub enum FreeRtosError {
    #[error("Insufficient Heap Memory.")]
    InsufficientHeapMemory = -1,
    #[error("Generic Failure.")]
    GenericFailure = 0,
    #[error("Unknown")]
    Unknown = -99,
    #[error("OK")]
    Ok = 1,
}

unsafe extern "C" {
    /// Delete a FreeRTOS task.
    pub fn vTaskDelete(task: *mut RtosTask);

    /// Notify a task to make progress outside of an interrupt.
    pub fn xTaskGenericNotify(
        task: *mut RtosTask,
        index: u32, // index to notify (usually 0)
        ul_value: u32,
        action: u32, // eNotifyAction (2 = eIncrement)
        previous_notification: *mut u32,
    ) -> i32;

    /// Notify a task to make progress when in an interrupt context.
    pub fn xTaskGenericNotifyFromISR(
        task: *mut c_void,
        index: u32,
        ul_value: u32,
        action: u32,
        previous_notification: *mut u32,
        pxHigherPriorityTaskWoken: *mut i32,
    ) -> i32;

    /// Wait a specified time or notification to make progress.
    pub fn ulTaskGenericNotifyTake(
        ux_index_to_wait_on: usize,
        x_clear_count_on_exit: i32,
        xTicksToWait: u32,
    ) -> u32;

    /// Get a pointer to the current task.
    pub fn xTaskGetCurrentTaskHandle() -> *mut RtosTask;

    /// Create a new FreeRTOS task.
    pub fn xTaskCreate(
        // Pointer to your extern "C" Rust function
        px_task_code: TaskFn,
        // Name of the task
        pc_name: *const c_char,
        // Stack depth in words
        us_stack_depth: u16,
        // Arguments to be passed to the task
        pv_parameters: *mut RtosTaskParams,
        // Task Priority
        ux_priority: u32,
        // Task Handle
        px_created_task: *mut RtosTask,
    ) -> FreeRtosError;

    pub fn xTaskCreateStatic(
        // Pointer to your extern "C" Rust function
        px_task_code: TaskFn,
        // Name of the task
        pc_name: *const c_char,
        // Stack depth in words
        us_stack_depth: u16,
        // Arguments to be passed to the task
        pv_parameters: *mut RtosTaskParams,
        // Task Priority
        ux_priority: u32,
        // Stack buffer
        stack_buf_ptr: *mut u8,
        // TCB buffer ~[0u8; 128]
        tcb_buf_ptr: *mut u8,
    ) -> *mut RtosTask;

    /// Delay a task.
    pub fn vTaskDelay(ticks: u32);

    /// Alloc some FreeRTOS managed heap memory.
    pub fn pvPortMalloc(size: usize) -> *mut u8;

    /// Free some FreeRTOS managed heap memory.
    pub fn vPortFree(ptr: *mut u8);

    /// Get the current time in ticks.
    pub fn xTaskGetTickCount() -> u32;

    /// Enter a critical section.
    pub fn vPortEnterCritical();

    /// Exit a critical section.
    pub fn vPortExitCritical();

    // Free heap size.
    pub fn xPortGetFreeHeapSize() -> usize;

    // Stack size
    pub fn uxTaskGetStackHighWaterMark(task_handle: *mut core::ffi::c_void) -> usize;
}
