use core::ffi::{CStr, c_char};

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
    unsafe { derusting_is_idle() }
}

/// The possible error when sending a gcode command.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Marlin returned false.")]
    MarlinReturnedFalse,
    #[error("Error build gcode: {0}.")]
    ExtendError(#[from] heapless::c_string::ExtendError),
    #[error("the printer is is busy.")]
    Busy,
}

/// All the prints setn through `derusting` are all dry print (no extrusion)
/// for demonstration purposes.
pub fn print(path: &CStr, dry: bool) -> Result<(), Error> {
    if is_idle() {
        log_info!("Dry Print Initiated");
        let mut cmd = CString::<64>::new();
        cmd.extend_from_bytes(b"M32 ")?;
        cmd.extend_from_bytes(path.to_bytes())?;
        if dry {
            let res = unsafe { derusting_gcode_cmd(c"M111 S8".as_ptr()) };
            if !res {
                return Err(Error::MarlinReturnedFalse);
            }
        }
        let res = unsafe { derusting_gcode_cmd(cmd.as_ptr()) };
        if res {
            Ok(())
        } else {
            Err(Error::MarlinReturnedFalse)
        }
    } else {
        Err(Error::Busy)
    }
}

/// Home the printer.
#[allow(unused)]
pub fn home() -> Result<(), Error> {
    if is_idle() {
        log_info!("Home called");
        let res = unsafe { derusting_gcode_cmd(c"G28".as_ptr()) };
        if res {
            Ok(())
        } else {
            Err(Error::MarlinReturnedFalse)
        }
    } else {
        Err(Error::Busy)
    }
}

/// A flag that enables a technician to say the printer is ready
/// to manufacture new jobs.
pub fn is_ready() -> bool {
    unsafe { derusting_ready_flag.load(core::sync::atomic::Ordering::SeqCst) }
}

/// We need to set the read_flag to false when a job has been selected by the
/// machine for manufacture.
pub fn set_offline() {
    unsafe { derusting_ready_flag.store(false, core::sync::atomic::Ordering::SeqCst) };
    unsafe { derusting_update_ui() };
}
