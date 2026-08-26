use core::ffi::{CStr, c_uint};

use alloc::boxed::Box;

mod bindings;

use bindings::*;

use crate::log_error;

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

pub struct File {
    inner: Box<core::mem::MaybeUninit<Fil>>,
    closed: bool,
}

impl File {
    pub fn open(path: &CStr, mode: FileMode) -> Option<Self> {
        let mut file = Box::<Fil>::new_uninit();
        let fp = file.as_mut_ptr();
        let res = unsafe { f_open(fp, path.as_ptr(), mode.bits()) };
        let res: Result<(), FileResult> = res.into();
        if let Err(e) = res {
            log_error!("File Error: {:?}", e);
            return None;
        }
        Some(Self {
            inner: file,
            closed: false,
        })
    }

    pub fn close(mut self) -> Result<(), FileResult> {
        let res = unsafe { f_close(self.as_mut_ptr()).into() };
        self.closed = true;
        res
    }

    fn as_mut_ptr(&mut self) -> *mut Fil {
        self.inner.as_mut_ptr()
    }
}

impl Drop for File {
    fn drop(&mut self) {
        if !self.closed {
            unsafe { f_close(self.as_mut_ptr()) };
        }
    }
}

impl embedded_io::ErrorType for File {
    type Error = FileResult;
}

impl embedded_io::Read for File {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let fp = self.as_mut_ptr();
        let mut bytes_read: c_uint = 0;
        let res = unsafe { f_read(fp, buf.as_mut_ptr(), buf.len() as c_uint, &mut bytes_read) };
        let res: Result<(), FileResult> = res.into();
        res?;
        Ok(bytes_read as usize)
    }
}

impl embedded_io::Write for File {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        let fp = self.as_mut_ptr();
        let mut bytes_written: c_uint = 0;
        let res = unsafe { f_write(fp, buf.as_ptr(), buf.len() as c_uint, &mut bytes_written) };
        let res: Result<(), FileResult> = res.into();
        res?;
        Ok(bytes_written as usize)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        let fp = self.as_mut_ptr();
        unsafe { f_sync(fp).into() }
    }
}
