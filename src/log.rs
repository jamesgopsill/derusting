use core::ffi::c_char;

use alloc::format;

/// The severity of the log event.
#[repr(i32)]
#[allow(unused)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Severity {
    Debug = 1,
    Info = 2,
    Warning = 3,
    Error = 4,
    Critical = 5,
}

// The `.cpp` hook exposes an extern "C" function to the
// firmware logger so we can hook into it.
unsafe extern "C" {
    pub fn derusting_log_event(severity: Severity, msg: *const c_char);
}

/// Our internal log function
pub fn log(severity: Severity, args: core::fmt::Arguments) {
    let mut msg = format!("{}", args);
    msg.push('\0');
    // SAFETY: `derusting_log_event` (see libderusting.cpp) forwards `msg`
    // to a "%s"-style logger, so it must point to a valid, nul-terminated
    // byte string for the duration of this call. We just appended '\0' to
    // `msg` above and the pointer stays valid until `msg` is dropped after
    // this call returns.
    unsafe { derusting_log_event(severity, msg.as_ptr() as *const _) };
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {{
        $crate::log::log($crate::log::Severity::Info, format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {{
        $crate::log::log($crate::log::Severity::Debug, format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {{
        $crate::log::log($crate::log::Severity::Error, format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! log_warning {
    ($($arg:tt)*) => {{
        $crate::log::log($crate::log::Severity::Warning, format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! log_critical {
    ($($arg:tt)*) => {{
        $crate::log::log($crate::log::Severity::Critical, format_args!($($arg)*));
    }};
}
