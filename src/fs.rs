use core::ffi::{CStr, c_char, c_int, c_long};

use crate::{log_error, log_info};
use alloc::string::ToString as _;
use embedded_io::Write as _;
use heapless::CString;
use uuid::Uuid;

/// Represents the `stdio.h` Fil struct. The Prusa firmware enables you
/// to use `stdio` calls to work with files. Under the hood it is using
/// chanfs.
#[repr(C)]
struct Fil {
    _opaque: [u8; 0],
}

/// Mirrors FatFs's FILINFO struct, for this firmware's config:
/// FF_FS_EXFAT=0 (fsize is u32), FF_USE_LFN=2 (altname+fname present,
/// stack-allocated working buffer), FF_LFN_UNICODE=2 (TCHAR=UTF-8 char),
/// FF_SFN_BUF=34, FF_LFN_BUF=255.
#[repr(C)]
pub struct FilInfo {
    pub fsize: u32,
    pub fdate: u16,
    pub ftime: u16,
    pub fattrib: u8,
    pub altname: [c_char; 35],
    pub fname: [c_char; 256],
}

/// Mirrors FatFs's `FRESULT` enum (see `ff.h`).
#[allow(unused)]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FResult {
    #[error("Succeeded")]
    Ok = 0,
    #[error("A hard error occurred in the low level disk I/O layer")]
    DiskErr = 1,
    #[error("Assertion failed (internal FatFs error)")]
    IntErr = 2,
    #[error("The physical drive cannot work")]
    NotReady = 3,
    #[error("Could not find the file")]
    NoFile = 4,
    #[error("Could not find the path")]
    NoPath = 5,
    #[error("The path name format is invalid")]
    InvalidName = 6,
    #[error("Access denied due to prohibited access or directory full")]
    Denied = 7,
    #[error("Access denied due to prohibited access")]
    Exist = 8,
    #[error("The file/directory object is invalid")]
    InvalidObject = 9,
    #[error("The physical drive is write protected")]
    WriteProtected = 10,
    #[error("The logical drive number is invalid")]
    InvalidDrive = 11,
    #[error("The volume has no work area")]
    NotEnabled = 12,
    #[error("There is no valid FAT volume")]
    NoFilesystem = 13,
    #[error("The f_mkfs() aborted due to any problem")]
    MkfsAborted = 14,
    #[error("Could not get a grant to access the volume within defined period")]
    Timeout = 15,
    #[error("The operation is rejected according to the file sharing policy")]
    Locked = 16,
    #[error("LFN working buffer could not be allocated")]
    NotEnoughCore = 17,
    #[error("Number of open files > FF_FS_LOCK")]
    TooManyOpenFiles = 18,
    #[error("Given parameter is invalid")]
    InvalidParameter = 19,
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

    /// Renames or moves a file.
    /// Returns 0 on success, or a non-zero value / -1 on failure.
    fn rename(oldpath: *const c_char, newpath: *const c_char) -> c_int;

    fn f_stat(path: *const c_char, fno: *mut FilInfo) -> FResult;
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

/// Open a file in a particular mode.
pub fn open<T>(path: &CStr, mode: T) -> Result<File<T>, ()>
where
    T: Mode,
{
    if !path.to_bytes().starts_with(b"/usb/") {
        log_error!("Path must start with /usb/");
        return Err(());
    }
    let res = unsafe { fopen(path.as_ptr(), mode.as_cstr().as_ptr()) };
    if res.is_null() {
        Err(())
    } else {
        Ok(File {
            fp: res,
            _mode: mode,
        })
    }
}

pub fn delete(path: &CStr) -> c_int {
    unsafe { unlink(path.as_ptr()) }
}

pub fn rname(old_path: &CStr, new_path: &CStr) -> c_int {
    unsafe { rename(old_path.as_ptr(), new_path.as_ptr()) }
}

pub fn stat(path: &CStr) -> Result<FilInfo, FResult> {
    let bytes = path.to_bytes();
    if !bytes.starts_with(b"/usb/") {
        log_error!("Path must start with /usb/");
        return Err(FResult::InvalidName);
    }

    // FatFs native API doesn't know the `/usb/` convenience prefix the
    // POSIX shim uses — it wants `0:/...` (drive 0, since FF_VOLUMES == 1
    // and FF_STR_VOLUME_ID == 0 rules out string IDs like "USB:").
    let rest = &bytes[b"/usb".len()..]; // keeps the leading '/'
    let mut native_path: CString<64> = CString::new();
    native_path
        .extend_from_bytes(b"0:")
        .map_err(|_| FResult::InvalidName)?;
    native_path
        .extend_from_bytes(rest)
        .map_err(|_| FResult::InvalidName)?;

    let mut info = core::mem::MaybeUninit::<FilInfo>::uninit();
    let res = unsafe { f_stat(native_path.as_ptr(), info.as_mut_ptr()) };

    if res != FResult::Ok {
        return Err(res);
    }

    let info = unsafe { info.assume_init() };
    Ok(info)
}

/// A Rust safe wrapper around a file.
pub struct File<T>
where
    T: Mode,
{
    fp: *mut Fil,
    _mode: T,
}

/// Functions available across all modes.
impl<T> File<T>
where
    T: Mode,
{
    /// Closes the file. The file will automatically be closed on drop but
    /// some might like to be explicit.
    pub fn close(self) {}
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
    if let Ok(mut f) = open(c"/usb/test.txt", WriteBytes) {
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

/// Create the full file path for a given uuid.
pub fn make_path(job_guid: &Uuid, partial: bool) -> CString<64> {
    let mut path = CString::<64>::new();
    let _ = path.extend_from_bytes(b"/usb/");
    let _ = path.extend_from_bytes(job_guid.to_string().as_bytes());
    if partial {
        let _ = path.extend_from_bytes(b".partial");
    } else {
        let _ = path.extend_from_bytes(b".gcode");
    }
    path
}
