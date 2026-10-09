use core::panic::PanicInfo;

// SAFETY: `abort` is provided by the firmware's C library and matches
// `void abort(void)`; it takes no arguments and never returns.
unsafe extern "C" {
    /// The firmware's `abort` routine, invoked to terminate on panic.
    fn abort() -> !;
}

/// We need a panic handler and here it is. It hooks into the
/// abort function present in the firmware.
#[panic_handler]
pub fn panic(_info: &PanicInfo) -> ! {
    // SAFETY: `abort` takes no arguments, has no preconditions and diverges,
    // matching the `!` return type of this handler.
    unsafe { abort() };
}
