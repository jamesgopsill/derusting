use core::alloc::{GlobalAlloc, Layout};

use super::bindings::*;

/// Hooking into FreeRTOS allocator to provide alloc.
pub struct FreeRtosAllocator;

// SAFETY: `pvPortMalloc`/`vPortFree` behave like a paired malloc/free (same
// heap, matching pointers), which is what `GlobalAlloc` requires. Note this
// does *not* honour `layout.align()` beyond whatever alignment FreeRTOS's
// heap implementation naturally provides (see the note in `alloc` below).
unsafe impl GlobalAlloc for FreeRtosAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: `pvPortMalloc` returns either null or a pointer to at
        // least `layout.size()` freeable bytes. Caller (the `alloc`
        // trait contract) requires the returned pointer to also satisfy
        // `layout.align()`; we rely on FreeRTOS's heap giving out memory
        // aligned to `portBYTE_ALIGNMENT` and do not further validate that.
        unsafe { pvPortMalloc(layout.size()) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        // SAFETY: caller guarantees `ptr` was returned by `alloc` above and
        // not already freed.
        unsafe { vPortFree(ptr) };
    }
}
