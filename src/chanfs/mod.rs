use core::{
    cell::RefCell,
    ffi::{CStr, c_uint},
    mem::MaybeUninit,
};

mod bindings;

use bindings::*;
use embassy_sync::blocking_mutex::{Mutex, raw::ThreadModeRawMutex};

use crate::{log_error, log_info};

static FILE: Mutex<ThreadModeRawMutex, RefCell<File>> = Mutex::new(RefCell::new(File {
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
        FILE.lock(|f| {
            let mut file = f.borrow_mut();
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
        FILE.lock(|f| {
            let mut file = f.borrow_mut();
            if !file.is_open {
                return Err(FileResult::NoFile);
            }
            let fp = file.inner.as_mut_ptr();
            let mut bytes_written: u32 = 0;
            let res = unsafe { f_write(fp, buf.as_ptr(), buf.len() as c_uint, &mut bytes_written) };
            if res != FileResult::Ok {
                return Err(res);
            }
            Ok(bytes_written as usize)
        })
    }

    fn _close(&self) -> Result<(), FileResult> {
        FILE.lock(|f| {
            let mut file = f.borrow_mut();
            if file.is_open {
                let fp = file.inner.as_mut_ptr();
                let res = unsafe { f_close(fp) };
                if res != FileResult::Ok {
                    log_error!("File Closed Error: {}", res);
                    return Err(res);
                }
            }
            file.is_open = false;
            Ok(())
        })
    }

    #[allow(unused)]
    pub fn close(self) -> Result<(), FileResult> {
        self._close()
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        log_info!("Drop Entered");
        let _ = self._close();
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
    }
}
