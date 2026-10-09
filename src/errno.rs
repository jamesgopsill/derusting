use core::ffi::c_int;

const ENOENT: c_int = 2;
const EIO: c_int = 5;
const EACCES: c_int = 13;
const EEXIST: c_int = 17;
const EINVAL: c_int = 22;
const ENOSPC: c_int = 28;

unsafe extern "C" {
    // newlib: `errno` is `(*__errno())`, per FreeRTOS task when newlib
    // reentrancy is enabled.
    pub fn __errno() -> *mut c_int;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error(c_int);

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl core::error::Error for Error {}

impl Error {
    pub const INVALID_INPUT: Self = Self(EINVAL);

    /// Read errno after a failed call. Falls back to EIO if the C side
    /// failed without setting errno.
    pub fn last() -> Self {
        match Self::get_errno() {
            0 => Self(EIO),
            e => Self(e),
        }
    }

    pub fn get_errno() -> c_int {
        unsafe { *__errno() }
    }

    pub fn clear_errno() {
        unsafe { *__errno() = 0 };
    }
}

impl embedded_io::Error for Error {
    fn kind(&self) -> embedded_io::ErrorKind {
        use embedded_io::ErrorKind::*;
        match self.0 {
            ENOENT => NotFound,
            EEXIST => AlreadyExists,
            EACCES => PermissionDenied,
            EINVAL => InvalidInput,
            ENOSPC => OutOfMemory,
            _ => Other,
        }
    }
}
