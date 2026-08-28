use core::{
    cell::RefCell,
    ffi::{CStr, c_uint},
    mem::MaybeUninit,
};

mod bindings;

use bindings::*;
use critical_section::Mutex;

use crate::{log_error, log_info};

static FILE: Mutex<RefCell<File>> = Mutex::new(RefCell::new(File {
    is_open: false,
    inner: MaybeUninit::uninit(),
}));

bitflags::bitflags! {
    pub struct FileMode: u8 {
        const READ          = 0x01;
        const WRITE         = 0x02;
        const OPEN_EXISTING = 0x00;
        const CREATE_NEW    = 0x04;
        const CREATE_ALWAYS = 0x08;
        const OPEN_ALWAYS   = 0x10;
        const OPEN_APPEND   = 0x30;
    }
}

struct File {
    is_open: bool,
    inner: MaybeUninit<Fil>,
}

pub struct FileLock();

impl FileLock {
    pub fn open(path: &CStr, mode: FileMode) -> Result<Self, FileResult> {
        critical_section::with(|cs| {
            let mut file = FILE.borrow(cs).borrow_mut();
            if file.is_open {
                return Err(FileResult::TooManyOpenFiles);
            }
            let fp = file.inner.as_mut_ptr();
            let res = unsafe { f_open(fp, path.as_ptr(), mode.bits()) };
            if res != FileResult::Ok {
                return Err(res);
            }
            file.is_open = true;
            Ok(Self())
        })
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize, FileResult> {
        let fp = critical_section::with(|cs| {
            let mut file = FILE.borrow(cs).borrow_mut();
            if !file.is_open {
                return Err(FileResult::NoFile);
            }
            Ok(file.inner.as_mut_ptr())
        })?;
        let mut bytes_written: c_uint = 0;
        let res = unsafe { f_write(fp, buf.as_ptr(), buf.len() as c_uint, &mut bytes_written) };
        if res != FileResult::Ok {
            return Err(res);
        }
        Ok(bytes_written as usize)
    }

    pub fn close(self) -> Result<(), FileResult> {
        critical_section::with(|cs| {
            let mut file = FILE.borrow(cs).borrow_mut();
            if file.is_open {
                return Ok(());
            }
            let fp = file.inner.as_mut_ptr();
            let res = unsafe { f_close(fp) };
            if res != FileResult::Ok {
                return Err(res);
            }
            file.is_open = false;
            Ok(())
        })
    }
}

pub fn test_file() {
    if let Ok(flock) = FileLock::open(
        c"test.txt",
        FileMode::READ | FileMode::WRITE | FileMode::CREATE_ALWAYS,
    ) {
        log_info!("Test File Opened");
        if let Err(e) = flock.write(b"Hello World\n") {
            log_error!("File write error: {e}");
        } else {
            log_info!("File Write Complete");
        };
        if let Err(e) = flock.close() {
            log_error!("File close error: {e}");
        } else {
            log_info!("File Closed");
        };
    }
}
