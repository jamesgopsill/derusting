use core::ffi::{c_char, c_uint};

use num_enum::FromPrimitive;

use crate::log_error;

unsafe extern "C" {
    pub fn f_open(fp: *mut Fil, path: *const c_char, mode: u8) -> FileResult;
    pub fn f_close(fp: *mut Fil) -> FileResult;
    #[allow(unused)]
    pub fn f_read(fp: *mut Fil, buff: *mut u8, btr: c_uint, br: *mut c_uint) -> FileResult;
    pub fn f_write(fp: *mut Fil, buff: *const u8, btw: c_uint, bw: *mut c_uint) -> FileResult;
    #[allow(unused)]
    pub fn f_lseek(fp: *mut Fil, ofs: c_uint) -> FileResult;
    #[allow(unused)]
    pub fn f_sync(fp: *mut Fil) -> FileResult;
}

#[repr(C)]
#[repr(align(8))]
pub struct Fil {
    _opaque: [u8; 768], // Opaque + padding to ensure there is enough space for the C struct
}

#[repr(u32)]
#[derive(Debug, PartialEq, Eq, FromPrimitive)]
pub enum FileResult {
    Ok = 0,
    DiskErr = 1,
    IntErr = 2,
    NotReady = 3,
    NoFile = 4,
    NoPath = 5,
    InvalidName = 6,
    Denied = 7,
    Exist = 8,
    InvalidObject = 9,
    WriteProtected = 10,
    InvalidDrive = 11,
    NotEnabled = 12,
    NoFileSystem = 13,
    MkfsAborted = 14,
    Timeout = 15,
    Locked = 16,
    NotEnoughCore = 17,
    TooManyOpenFiles = 18,
    InvalidParameter = 19,
    #[num_enum(catch_all)]
    Unknown(u32),
}

#[allow(clippy::from_over_into)]
impl Into<Result<(), FileResult>> for FileResult {
    fn into(self) -> Result<(), FileResult> {
        if self == FileResult::Ok {
            Ok(())
        } else {
            log_error!("FileResult Error: {:?}", self);
            Err(self)
        }
    }
}

impl core::fmt::Display for FileResult {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let msg = match self {
            Self::Ok => "Succeeded",
            Self::DiskErr => "A hard error occurred in the low level disk I/O layer",
            Self::IntErr => "Assertion failed",
            Self::NotReady => "The physical drive cannot work",
            Self::NoFile => "Could not find the file",
            Self::NoPath => "Could not find the path",
            Self::InvalidName => "The path name format is invalid",
            Self::Denied => "Access denied due to prohibited access or directory full",
            Self::Exist => "Access denied due to prohibited access (object exists)",
            Self::InvalidObject => "The file/directory object is invalid",
            Self::WriteProtected => "The physical drive is write protected",
            Self::InvalidDrive => "The logical drive number is invalid",
            Self::NotEnabled => "The volume has no work area",
            Self::NoFileSystem => "There is no valid FAT volume",
            Self::MkfsAborted => "The f_mkfs() aborted due to a parameter error",
            Self::Timeout => "Could not get a grant to access the volume (timeout)",
            Self::Locked => "The operation is rejected according to the file sharing policy",
            Self::NotEnoughCore => "LFN working buffer could not be allocated",
            Self::TooManyOpenFiles => "Number of open files > FF_FS_LOCK",
            Self::InvalidParameter => "Given parameter is invalid",
            Self::Unknown(code) => {
                return write!(f, "Unknown FatFs error code: {}", code);
            }
        };
        f.write_str(msg)
    }
}

impl core::error::Error for FileResult {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        None
    }

    fn description(&self) -> &str {
        "description() is deprecated; use Display"
    }

    fn cause(&self) -> Option<&dyn core::error::Error> {
        self.source()
    }
}

impl embedded_io::Error for FileResult {
    fn kind(&self) -> embedded_io::ErrorKind {
        match self {
            Self::Ok => embedded_io::ErrorKind::Other,
            Self::NoFile | Self::NoPath => embedded_io::ErrorKind::NotFound,
            Self::Denied => embedded_io::ErrorKind::PermissionDenied,
            Self::Exist => embedded_io::ErrorKind::AlreadyExists,
            Self::Timeout => embedded_io::ErrorKind::TimedOut,
            Self::InvalidParameter => embedded_io::ErrorKind::InvalidInput,
            _ => embedded_io::ErrorKind::Other,
        }
    }
}
