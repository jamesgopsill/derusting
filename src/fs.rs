use core::ffi::{CStr, c_char, c_int, c_long};

use crate::{log_error, log_info};
use embedded_io::Write as _;

/// Represents the `stdio.h` Fil struct. The Prusa firmware enables you
/// to use `stdio` calls to work with files. Under the hood it is using
/// chanfs.
#[repr(C)]
struct Fil {
    _opaque: [u8; 0],
}

unsafe extern "C" {
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
}

/// A trait the produces the necesary flags for `fopen`
pub trait Mode {
    fn as_cstr(&self) -> &'static CStr;
}

// A marker trait implements read
pub trait ImplementsEmbeddedIoRead {}

// A marker trait for a mode the implements
pub trait ImplementsEmbeddedIoWrite {}

/// Marker for setting the file to read-only.
pub struct ReadBytes;

impl Mode for ReadBytes {
    fn as_cstr(&self) -> &'static CStr {
        c"rb"
    }
}

// Read Bytes implements `embedded::io::Read`.
impl ImplementsEmbeddedIoRead for ReadBytes {}

/// Marker for setting the file to read-write.
pub struct WriteBytes;

impl Mode for WriteBytes {
    fn as_cstr(&self) -> &'static CStr {
        c"wb"
    }
}

// Read Bytes implements `embedded::io::Write`.
impl ImplementsEmbeddedIoWrite for WriteBytes {}

/// A Rust safe wrapper around a file.
pub struct File<T>
where
    T: Mode,
{
    fp: *mut Fil,
    _mode: T,
}

/// Functions available across all modes.
impl<T: Mode> File<T> {
    /// Open a file in a particular mode.
    pub fn open(path: &CStr, mode: T) -> Result<Self, ()> {
        if !path.to_bytes().starts_with(b"/usb/") {
            log_error!("Path must start with /usb/");
            return Err(());
        }
        let res = unsafe { fopen(path.as_ptr(), mode.as_cstr().as_ptr()) };
        if res.is_null() {
            Err(())
        } else {
            Ok(Self {
                fp: res,
                _mode: mode,
            })
        }
    }

    /// Closes the file. The file will automatically be closed on drop but
    /// some might like to be explicit.
    pub fn close(self) {}

    pub fn delete(path: &CStr) -> c_int {
        unsafe { unlink(path.as_ptr()) }
    }
}

/// The error enum for File
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Internal ChanFS/stdio error.")]
    IoError,
}

impl embedded_io::Error for Error {
    fn kind(&self) -> embedded_io::ErrorKind {
        embedded_io::ErrorKind::Other
    }
}

impl<T: Mode> embedded_io::ErrorType for File<T> {
    type Error = Error;
}

/// Implements `embedded::io::Read` across any `Mode` that is marked that
/// it implements `ImplementsEmbeddedIoRead`.
impl<T: Mode + ImplementsEmbeddedIoRead> embedded_io::Read for File<T> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let read = unsafe { fread(buf.as_mut_ptr(), 1, buf.len(), self.fp) };
        Ok(read)
    }
}

/// Implements `embedded::io::Write` across any `Mode` that is marked that
/// it implements `ImplementsEmbeddedIoWrite`.
impl<T: Mode + ImplementsEmbeddedIoWrite> embedded_io::Write for File<T> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        let written = unsafe { fwrite(buf.as_ptr(), 1, buf.len(), self.fp) };
        Ok(written)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        let res = unsafe { fflush(self.fp) };
        if res > 0 {
            Err(Self::Error::IoError)
        } else {
            Ok(())
        }
    }
}

/// Implements `embedded::io::Seek`  for any `Mode`.
impl<T: Mode> embedded_io::Seek for File<T> {
    fn seek(&mut self, pos: embedded_io::SeekFrom) -> Result<u64, Self::Error> {
        let (offset, whence) = match pos {
            embedded_io::SeekFrom::Start(o) => (o as c_long, 0), // SEEK_SET
            embedded_io::SeekFrom::Current(o) => (o as c_long, 1), // SEEK_CUR
            embedded_io::SeekFrom::End(o) => (o as c_long, 2),   // SEEK_END
        };

        if unsafe { fseek(self.fp, offset, whence) } != 0 {
            return Err(Error::IoError);
        }

        let tell = unsafe { ftell(self.fp) };
        if tell < 0 {
            Err(Error::IoError)
        } else {
            Ok(tell as u64)
        }
    }
}

/// Implements drop for file ensuring the file is closed.
impl<T> Drop for File<T>
where
    T: Mode,
{
    fn drop(&mut self) {
        unsafe { fclose(self.fp) };
    }
}

#[allow(unused)]
pub fn test_file() {
    if let Ok(mut f) = File::open(c"/usb/test.txt", WriteBytes) {
        log_info!("Test File Opened");
        match f.write(b"Hello World\n") {
            Ok(written) => {
                log_info!("Bytes written: {written}");
            }
            Err(_) => {
                log_error!("Failed to write bytes");
            }
        };
        f.close();
        log_info!("Test File Closed");
    }
}
