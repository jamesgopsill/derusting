use core::panic::PanicInfo;

unsafe extern "C" {
    /// The firmware's `abort` routine, invoked to terminate on panic.
    fn abort() -> !;
}

/// We need a panic handler and here it is. It hooks into the
/// abort function present in the firmware.
#[panic_handler]
pub fn panic(_info: &PanicInfo) -> ! {
    unsafe { abort() };
}
