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
    fn fclose(fp: *mut Fil) -> c_int;

    /// Seek to a position along a file.
    fn fseek(fp: *mut Fil, offset: c_long, whence: c_int) -> c_int;

    // Tell us where we are.
    fn ftell(fp: *mut Fil) -> c_long;

    // Flush the file to disk
    fn fflush(fp: *mut Fil) -> c_int;

    /// Delete a file
    fn unlink(path: *const c_char) -> c_int;

    /// Returns the error
    fn ferror(fp: *mut Fil) -> c_int;

    /// Renames or moves a file.
    /// Returns 0 on success, or a non-zero value / -1 on failure.
    fn rename(oldpath: *const c_char, newpath: *const c_char) -> c_int;
}

/// Represents the `stdio.h` Fil struct.
#[repr(C)]
struct Fil {
    _opaque: [u8; 0],
}

pub struct File(*mut Fil);

impl embedded_io::ErrorType for File {
    type Error = crate::errno::Error;
}

impl embedded_io::Write for File {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        Self::Error::clear_errno();

        // SAFETY: `self.fp` is a live `Fil*` for as long as `self` exists,
        // and `buf` is a valid, readable slice of `buf.len()` bytes.
        let n = unsafe { fwrite(buf.as_ptr(), 1, buf.len(), self.0) };
        if n == 0 {
            return Err(Self::Error::last());
        }
        Ok(n)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Self::Error::clear_errno();
        // SAFETY: `self.fp` is a live `Fil*` for as long as `self` exists.
        if unsafe { fflush(self.0) } != 0 {
            Err(Self::Error::last())
        } else {
            Ok(())
        }
    }
}

impl embedded_io::Read for File {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        Self::Error::clear_errno();
        // SAFETY: `self.fp` is a live `Fil*` for as long as `self` exists
        // (only closed in `Drop`), and `buf` is a valid, writable slice of
        // `buf.len()` bytes that `fread` will write at most that many
        // bytes into.
        let n = unsafe { fread(buf.as_mut_ptr(), 1, buf.len(), self.0) };
        if n == 0 && unsafe { ferror(self.0) } != 0 {
            return Err(Self::Error::last());
        }
        Ok(n)
    }
}

impl crate::service::Vfs for File {
    async fn open(path: &str, flag: VfsFlag) -> Result<Self, Self::Error> {
        let path = c_path(path)?;
        // SAFETY: `path` and `mode.as_cstr()` are both valid, nul-terminated C
        // strings for the duration of the call. `fopen` returns either null or
        // a pointer owned by the C runtime that we take ownership of via `File`
        // (closed in `File::drop`);
        let res = match flag {
            VfsFlag::Write => unsafe { fopen(path.as_ptr(), c"wb".as_ptr()) },
            VfsFlag::Read => unsafe { fopen(path.as_ptr(), c"rb".as_ptr()) },
        };
        if res.is_null() {
            Err(Self::Error::INVALID_INPUT)
        } else {
            Ok(File(res))
        }
    }

    async fn delete(path: &str) -> Result<(), Self::Error> {
        let path = c_path(path)?;
        // SAFETY: `path` is a valid, nul-terminated C string for the call.
        unsafe { unlink(path.as_ptr()) };
        Ok(())
    }

    async fn rename(src: &str, dest: &str) -> Result<(), Self::Error> {
        let src = c_path(src)?;
        let dest = c_path(dest)?;
        // SAFETY: `old_path`/`new_path` are valid, nul-terminated C strings.
        let _ = unsafe { rename(src.as_ptr(), dest.as_ptr()) };
        // TODO. Check the error
        Ok(())
    }
}

impl Drop for File {
    fn drop(&mut self) {
        use crate::errno::Error;
        // SAFETY: `self.fp` is a live `Fil*` opened by `open()` and not yet
        // closed (this is the only place that closes it); `File` is not
        // `Copy`/`Clone` so this runs at most once per underlying handle.
        Error::clear_errno();
        if unsafe { fclose(self.0) } != 0 {
            let err = Error::last();
            log_error!("File close err: {err}");
        }
    }
}

fn c_path(path: &str) -> Result<heapless::CString<64>, crate::errno::Error> {
    use crate::errno::Error;
    if !path.starts_with("/usb/") || path.split('/').any(|s| s == "..") {
        return Err(Error::INVALID_INPUT);
    }
    let mut c = heapless::CString::new();
    c.extend_from_bytes(path.as_bytes())
        .map_err(|_| Error::INVALID_INPUT)?;
    Ok(c)
}
