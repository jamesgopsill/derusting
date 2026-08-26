#![allow(unused)]
use core::ffi::{c_char, c_void};

pub type RtosTask = c_void;
pub type RtosTaskParams = c_void;
pub type TaskFn = unsafe extern "C" fn(*mut RtosTaskParams) -> !;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
#[allow(unused)]
pub enum FreeRtosError {
    InsufficientHeapMemory = -1,
    GenericFailure = 0,
    Unknown = -99,
    Ok = 1,
}

unsafe extern "C" {
    pub fn vTaskDelete(task: *mut RtosTask);

    pub fn xTaskGenericNotify(
        task: *mut RtosTask,
        index: u32,  // index to notify (usually 0)
        action: u32, // eNotifyAction (2 = eIncrement)
        previous_notification: *mut u32,
    ) -> i32;

    pub fn xTaskGenericNotifyFromISR(
        task: *mut c_void,
        index: u32,
        action: u32,
        previous_notification: *mut u32,
        pxHigherPriorityTaskWoken: *mut i32,
    ) -> i32;

    pub fn ulTaskGenericNotifyTake(
        ux_index_to_wait_on: usize,
        x_clear_count_on_exit: i32,
        xTicksToWait: u32,
    ) -> u32;

    pub fn xTaskGetCurrentTaskHandle() -> *mut c_void;

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

    pub fn vTaskDelay(ticks: u32);

    pub fn pvPortMalloc(size: usize) -> *mut u8;

    pub fn vPortFree(ptr: *mut u8);

    pub fn xTaskGetTickCount() -> u32;

    pub fn vPortEnterCritical();
    pub fn vPortExitCritical();
}
