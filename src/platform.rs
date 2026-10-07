use core::ffi::c_char;

use heapless::CString;

use crate::log_info;

// Extern "C" functions exposed by our glue layer - `libderusting.cpp`.
unsafe extern "C" {
    /// Submit aline of gcode to Marlin.
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

#[derive(Debug, Default)]
pub struct Platform {}

impl crate::service::Platform for Platform {
    fn is_available(&self) -> bool {
        is_ready() && is_idle()
    }

    fn manufacture(&self, guid: uuid::Uuid) -> bool {
        if self.is_available() {
            log_info!("Dry Print Initiated");
            let cmd = heapless::format!(64; "M32 /usb/{}.gcode\0", guid).unwrap();
            // SAFETY: `c"M111 S8"` is a `'static` nul-terminated C string
            // literal, valid for the duration of the call.
            let res = unsafe { derusting_gcode_cmd(c"M111 S8".as_ptr()) };
            // SAFETY: `cmd` is a `heapless::CString` we just built above; its
            // buffer is nul-terminated and remains valid for this call.
            unsafe { derusting_gcode_cmd(cmd.as_ptr()) }
        } else {
            false
        }
    }

    fn log(&self, msg: &str) {
        log_info!("{msg}");
    }
}
