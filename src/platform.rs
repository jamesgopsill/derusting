use core::ffi::c_char;

use crate::{log_info, lwip};

// Extern "C" functions exposed by our glue layer - `libderusting.cpp`.
//
// SAFETY: the declarations must match the glue layer's definitions.
// `derusting_ready_flag` is a single linker-provided atomic, so it is only
// accessed through atomic operations.
unsafe extern "C" {
    /// Submit a line of gcode to Marlin.
    fn derusting_gcode_cmd(cmd: *const c_char) -> bool;
    /// Check if the printer is idle.
    fn derusting_is_idle() -> bool;
    /// Our addition to the codebase to enable a "Ready" state that can
    /// be turned on by the technician on the GUI.
    static derusting_ready_flag: core::sync::atomic::AtomicBool;
    /// Refresh the printer UI if we have updated the ready_flag.
    fn derusting_update_ui();
}

/// Is the printer idling.
pub fn is_idle() -> bool {
    // SAFETY: `derusting_is_idle` takes no arguments and just reads
    // Marlin's internal state; safe to call from any context.
    unsafe { derusting_is_idle() }
}

/// A flag that enables a technician to say the printer is ready
/// to manufacture new jobs.
pub fn is_ready() -> bool {
    // SAFETY: `derusting_ready_flag` is a `static` `AtomicBool` defined on
    // the C++ side; referencing a `static` atomic across an FFI boundary is
    // sound as long as it is defined exactly once and never moved, which is
    // guaranteed for a linker-provided static.
    unsafe { derusting_ready_flag.load(core::sync::atomic::Ordering::SeqCst) }
}

/// The Prusa Buddy firmware implementation of the service's `Platform`
/// trait, backed by the C++ glue layer.
#[derive(Debug, Default)]
pub struct Platform {}

impl crate::service::Platform for Platform {
    /// True when the technician has marked the printer ready and Marlin is idle.
    fn is_available(&self) -> bool {
        is_ready() && is_idle()
    }

    /// Starts a print of `/usb/<guid>.gcode` if the printer is available.
    /// Returns whether Marlin accepted the command; on success the ready
    /// flag is cleared and the UI refreshed.
    fn manufacture(&self, guid: uuid::Uuid) -> bool {
        if self.is_available() {
            log_info!("Dry Print Initiated");
            let cmd = heapless::format!(64; "M32 /usb/{}.gcode\0", guid).unwrap();
            // SAFETY: `c"M111 S8"` is a `'static` nul-terminated C string
            // literal, valid for the duration of the call.
            let _ = unsafe { derusting_gcode_cmd(c"M111 S8".as_ptr()) };
            // SAFETY: `cmd` is a `heapless::String<64>` built above with an
            // explicit trailing `\0`, so its buffer is nul-terminated and
            // remains valid for this call.
            let res = unsafe { derusting_gcode_cmd(cmd.as_ptr()) };
            if res {
                // SAFETY: see `is_ready` above for why referencing this static is sound.
                unsafe { derusting_ready_flag.store(false, core::sync::atomic::Ordering::SeqCst) };
                // SAFETY: `derusting_update_ui` takes no arguments; it is documented as
                // safe to call whenever the ready flag has changed.
                unsafe { derusting_update_ui() };
            }
            res
        } else {
            false
        }
    }

    /// Logs `args` through the firmware logger at `Info` severity.
    fn log(&self, args: core::fmt::Arguments) {
        log_info!("{}", args);
    }

    /// The printer's local IPv4 address from lwIP, if it has one.
    fn local(&self) -> Option<core::net::Ipv4Addr> {
        lwip::local_ipv4()
    }
}
