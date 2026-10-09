use core::ffi::{c_char, c_int, c_long};

use crate::{log_error, service::VfsFlag};

unsafe extern "C" {
    #![allow(unused)]
    /// Opens a file.
    fn fopen(path: *const c_char, flags: *const c_char) -> *mut Fil;

    /// Write to a file.
    fn fwrite(buf_ptr: *const u8, buf_item_byte_size: usize, buf_len: usize, fp: *mut Fil)
    -> usize;

    /// Read from a file.
    fn fread(buf_ptr: *mut u8, buf_item_byte_size: usize, buf_len: usize, fp: *mut Fil) -> usize;

    /// Close a file.
    fn fclose(fp: *mut Fil);

    /// Seek to a position along a file.
    fn fseek(fp: *mut Fil, offset: c_long, whence: c_int) -> c_int;

    // Tell us where we are.
    fn ftell(fp: *mut Fil) -> c_long;

    // Flush the file to disk
    fn fflush(fp: *mut Fil) -> c_int;

    /// Delete a file
    fn unlink(path: *const c_char) -> c_int;

    /// Renames or moves a file.
    /// Returns 0 on success, or a non-zero value / -1 on failure.
    fn rename(oldpath: *const c_char, newpath: *const c_char) -> c_int;
}

/// Represents the `stdio.h` Fil struct.
#[repr(C)]
struct Fil {
    _opaque: [u8; 0],
}

#[derive(Debug)]
pub struct Error(c_int);

impl Error {
    const INVALID_INPUT: Self = Self(6);
    const OTHER: Self = Self(1);
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl core::error::Error for Error {}

impl embedded_io::Error for Error {
    fn kind(&self) -> embedded_io::ErrorKind {
        // TODO: finish
        match self.0 {
            1 => embedded_io::ErrorKind::Other,
            6 => embedded_io::ErrorKind::InvalidInput,
            _ => embedded_io::ErrorKind::Other,
        }
    }
}

pub struct File(*mut Fil);

impl embedded_io::ErrorType for File {
    type Error = Error;
}

impl embedded_io::Write for File {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        // SAFETY: `self.fp` is a live `Fil*` for as long as `self` exists,
        // and `buf` is a valid, readable slice of `buf.len()` bytes.
        let written = unsafe { fwrite(buf.as_ptr(), 1, buf.len(), self.0) };
        Ok(written)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        // SAFETY: `self.fp` is a live `Fil*` for as long as `self` exists.
        let res = unsafe { fflush(self.0) };
        if res > 0 {
            Err(Self::Error::OTHER)
        } else {
            Ok(())
        }
    }
}

impl embedded_io::Read for File {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        // SAFETY: `self.fp` is a live `Fil*` for as long as `self` exists
        // (only closed in `Drop`), and `buf` is a valid, writable slice of
        // `buf.len()` bytes that `fread` will write at most that many
        // bytes into.
        let read = unsafe { fread(buf.as_mut_ptr(), 1, buf.len(), self.0) };
        Ok(read)
    }
}

impl crate::service::Vfs for File {
    async fn open(path: &str, flag: VfsFlag) -> Result<Self, Self::Error> {
        if !path.starts_with("/usb/") {
            log_error!("Path must start with /usb/");
            return Err(Self::Error::INVALID_INPUT);
        }
        let mut c = heapless::CString::<64>::new();
        if c.extend_from_bytes(path.as_bytes()).is_err() {
            return Err(Error::INVALID_INPUT);
        };
        // SAFETY: `path` and `mode.as_cstr()` are both valid, nul-terminated C
        // strings for the duration of the call. `fopen` returns either null or
        // a pointer owned by the C runtime that we take ownership of via `File`
        // (closed in `File::drop`);
        let res = match flag {
            VfsFlag::Write => unsafe { fopen(c.as_ptr(), c"wb".as_ptr()) },
            VfsFlag::Read => unsafe { fopen(c.as_ptr(), c"rb".as_ptr()) },
        };
        if res.is_null() {
            Err(Self::Error::INVALID_INPUT)
        } else {
            Ok(File(res))
        }
    }

    async fn delete(path: &str) -> Result<(), Self::Error> {
        // SAFETY: `path` is a valid, nul-terminated C string for the call.
        if !path.starts_with("/usb/") {
            log_error!("Path must start with /usb/");
            return Err(Self::Error::INVALID_INPUT);
        }
        let mut c = heapless::CString::<64>::new();
        if c.extend_from_bytes(path.as_bytes()).is_err() {
            return Err(Self::Error::INVALID_INPUT);
        };
        unsafe { unlink(c.as_ptr()) };
        Ok(())
    }

    async fn rename(src: &str, dest: &str) -> Result<(), Self::Error> {
        if !src.starts_with("/usb/") {
            log_error!("Path must start with /usb/");
            return Err(Self::Error::INVALID_INPUT);
        }
        if !dest.starts_with("/usb/") {
            log_error!("Path must start with /usb/");
            return Err(Self::Error::INVALID_INPUT);
        }
        let mut c_src = heapless::CString::<64>::new();
        if c_src.extend_from_bytes(src.as_bytes()).is_err() {
            return Err(Self::Error::INVALID_INPUT);
        };
        let mut c_dest = heapless::CString::<64>::new();
        if c_dest.extend_from_bytes(dest.as_bytes()).is_err() {
            return Err(Self::Error::INVALID_INPUT);
        };
        // SAFETY: `old_path`/`new_path` are valid, nul-terminated C strings.
        let _ = unsafe { rename(c_src.as_ptr(), c_dest.as_ptr()) };
        // TODO. Check the error
        Ok(())
    }
}

impl Drop for File {
    fn drop(&mut self) {
        // SAFETY: `self.fp` is a live `Fil*` opened by `open()` and not yet
        // closed (this is the only place that closes it); `File` is not
        // `Copy`/`Clone` so this runs at most once per underlying handle.
        unsafe { fclose(self.0) };
    }
}
