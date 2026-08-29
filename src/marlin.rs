use core::ffi::c_char;

use crate::log_info;

// #include "marlin_client.hpp"

unsafe extern "C" {
    fn derusting_gcode_cmd(cmd: *const c_char) -> bool;
    fn derusting_is_idle() -> bool;
}

pub fn is_idle() -> bool {
    unsafe { derusting_is_idle() }
}

pub fn dry_print() {
    if is_idle() {
        log_info!("Dry Print Initiated");
        //unsafe { derusting_gcode_cmd(c"G98".as_ptr()) }; // Farm mode
        unsafe { derusting_gcode_cmd(c"M111 S8".as_ptr()) }; // Dry Print
        unsafe { derusting_gcode_cmd(c"M32 /usb/rust.gcode".as_ptr()) };
    }
}

pub fn home() {
    if is_idle() {
        log_info!("Home called");
        unsafe { derusting_gcode_cmd(c"G28".as_ptr()) };
    }
}
