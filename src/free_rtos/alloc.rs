use core::alloc::{GlobalAlloc, Layout};

use super::bindings::*;

/// Hooking into FreeRTOS allocator to provide alloc.
pub struct FreeRtosAllocator;

// SAFETY: `pvPortMalloc`/`vPortFree` behave like a paired malloc/free (same
// heap, matching pointers), which is what `GlobalAlloc` requires. Alignment
// is handled in `alloc` below: requests stricter than FreeRTOS's heap
// guarantees are refused by returning null.
unsafe impl GlobalAlloc for FreeRtosAllocator {
    /// Allocates `layout.size()` bytes from the FreeRTOS heap, or returns null
    /// on exhaustion or if `layout.align()` is greater than 8.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // FreeRTOS's heap only guarantees portBYTE_ALIGNMENT (8 bytes on
        // Cortex-M); refuse anything stricter rather than silently
        // returning under-aligned memory.
        if layout.align() > 8 {
            return core::ptr::null_mut();
        }
        // SAFETY: `pvPortMalloc` returns either null or a pointer to at
        // least `layout.size()` freeable bytes. The `GlobalAlloc` contract
        // also requires `layout.align()`; we rely on FreeRTOS's heap giving
        // out memory aligned to `portBYTE_ALIGNMENT` (checked above to be
        // enough) and do not further validate that.
        unsafe { pvPortMalloc(layout.size()) }
    }

    /// Returns `ptr` to the FreeRTOS heap.
    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        // SAFETY: caller guarantees `ptr` was returned by `alloc` above and
        // not already freed.
        unsafe { vPortFree(ptr) };
    }
}
