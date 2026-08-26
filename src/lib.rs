#![no_std]

use crate::free_rtos_alloc::FreeRtosAllocator;

extern crate alloc;

mod free_rtos_alloc;
mod log;
mod panic;

#[global_allocator]
static ALLOCATOR: FreeRtosAllocator = FreeRtosAllocator;

/// # Safety
/// We will ensure that we call this function in an
/// appropriate place in the Buddy firmware.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn derusting_main() {
    log_info!("Hello from Rust");
}
