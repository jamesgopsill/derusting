use core::ffi::c_char;

use crate::log_info;

unsafe extern "C" {
    fn derusting_gcode_cmd(cmd: *const c_char);
}

pub fn dry_print() {
    log_info!("Dry Print Initiated");
    unsafe { derusting_gcode_cmd(c"M111 S8".as_ptr()) };
    unsafe { derusting_gcode_cmd(c"M32 /usb/rust.gcode".as_ptr()) };
}

pub fn home() {
    log_info!("Home called");
    unsafe { derusting_gcode_cmd(c"G28".as_ptr()) };
}
