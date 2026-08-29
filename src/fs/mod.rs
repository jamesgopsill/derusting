mod bindings;

use core::ffi::CStr;

use bindings::*;

use crate::{log_error, log_info};

pub struct File {
    fp: *mut Fil,
    closed: bool,
}

impl File {
    pub fn open(path: &CStr, flags: &CStr) -> Result<Self, ()> {
        let res = unsafe { fopen(path.as_ptr(), flags.as_ptr()) };
        if res.is_null() {
            log_error!("File Open: NULL ptr");
            Err(())
        } else {
            Ok(Self {
                fp: res,
                closed: false,
            })
        }
    }

    #[allow(unused)]
    pub fn read(&self, buffer: &mut [u8]) -> usize {
        unsafe { fread(buffer.as_mut_ptr(), 1, buffer.len(), self.fp) }
    }

    pub fn write(&self, buf: &[u8]) -> usize {
        unsafe { fwrite(buf.as_ptr(), 1, buf.len(), self.fp) }
    }

    fn _close(&mut self) {
        if !self.closed {
            unsafe { fclose(self.fp) };
            self.closed = true;
        }
    }

    pub fn close(mut self) {
        self._close()
    }
}

// Automatically close the file when the variable goes out of scope
impl Drop for File {
    fn drop(&mut self) {
        self._close();
    }
}

pub fn test_file() {
    if let Ok(f) = File::open(c"/usb/test.txt", c"wb") {
        log_info!("Test File Opened");
        let written = f.write(b"Hello World\n");
        log_info!("Bytes written: {written}");
        f.close();
        log_info!("Test File Closed");
    }
}
