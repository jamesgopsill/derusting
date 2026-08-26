use core::alloc::{GlobalAlloc, Layout};

unsafe extern "C" {
    pub fn pvPortMalloc(size: usize) -> *mut u8;
    pub fn vPortFree(ptr: *mut u8);
}

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
