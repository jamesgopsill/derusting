use core::ffi::c_int;

const ENOENT: c_int = 2;
const EIO: c_int = 5;
const EACCES: c_int = 13;
const EEXIST: c_int = 17;
const EINVAL: c_int = 22;
const ENOSPC: c_int = 28;

// SAFETY: the declaration below must match newlib's `int *__errno(void)`
// signature; the symbol is provided by the firmware's C library at link time.
unsafe extern "C" {
    // newlib: `errno` is `(*__errno())`, per FreeRTOS task when newlib
    // reentrancy is enabled.
    pub fn __errno() -> *mut c_int;
}

/// A C `errno` value captured after a failed libc/lwIP call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error(c_int);

impl core::fmt::Display for Error {
    /// Formats the error using its `Debug` representation (the raw errno).
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl core::error::Error for Error {}

impl Error {
    /// Error value for invalid arguments (`EINVAL`), for failures detected
    /// on the Rust side before reaching C.
    pub const INVALID_INPUT: Self = Self(EINVAL);

    /// Read errno after a failed call. Falls back to EIO if the C side
    /// failed without setting errno.
    pub fn last() -> Self {
        match Self::get_errno() {
            0 => Self(EIO),
            e => Self(e),
        }
    }

    /// Returns the current value of the calling task's C `errno`.
    pub fn get_errno() -> c_int {
        // SAFETY: `__errno()` takes no arguments and returns a non-null
        // pointer to the calling task's errno slot (newlib reentrancy), which
        // is valid for reads while this task runs.
        unsafe { *__errno() }
    }

    /// Resets the calling task's C `errno` to 0, so a later failure can be
    /// told apart from a stale value.
    pub fn clear_errno() {
        // SAFETY: as in `get_errno`, the pointer from `__errno()` is valid
        // for writes for the calling task; writing 0 is a valid `errno`.
        unsafe { *__errno() = 0 };
    }
}

impl embedded_io::Error for Error {
    /// Maps the errno onto the closest `embedded_io::ErrorKind`; anything
    /// unrecognised becomes `Other`.
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
