use core::alloc::{GlobalAlloc, Layout};

use super::bindings::*;

/// Hooking into FreeRTOS allocator to provide alloc.
pub struct FreeRtosAllocator;

unsafe impl GlobalAlloc for FreeRtosAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { pvPortMalloc(layout.size()) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        unsafe { vPortFree(ptr) };
    }
}
