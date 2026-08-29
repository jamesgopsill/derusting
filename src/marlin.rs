use core::ffi::c_char;

use crate::log_info;

// #include "marlin_client.hpp"

unsafe extern "C" {
    fn derusting_gcode_cmd(cmd: *const c_char) -> bool;
    fn derusting_is_idle() -> bool;
    static derusting_ready_flag: core::sync::atomic::AtomicBool;
    fn derusting_update_ui();
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

#[allow(unused)]
pub fn home() {
    if is_idle() {
        log_info!("Home called");
        unsafe { derusting_gcode_cmd(c"G28".as_ptr()) };
    }
}

pub fn is_ready() -> bool {
    unsafe { derusting_ready_flag.load(core::sync::atomic::Ordering::SeqCst) }
}

pub fn set_offline() {
    unsafe { derusting_ready_flag.store(false, core::sync::atomic::Ordering::SeqCst) };
    unsafe { derusting_update_ui() };
}
