use core::ffi::{CStr, c_char};

use crate::{log_error, log_info};

// src/resources/revision.cpp

#[repr(C)]
pub(super) struct Fil {
    _opaque: [u8; 0],
}

unsafe extern "C" {
    pub(super) fn fopen(path: *const c_char, flags: *const c_char) -> *mut Fil;
    pub(super) fn fwrite(
        buf_ptr: *const u8,
        buf_item_byte_size: usize,
        buf_len: usize,
        fp: *mut Fil,
    ) -> usize;
    pub(super) fn fread(
        buf_ptr: *mut u8,
        buf_item_byte_size: usize,
        buf_len: usize,
        fp: *mut Fil,
    ) -> usize;
    pub(super) fn fclose(fp: *mut Fil);
}

pub struct File {
    fp: *mut Fil,
}

pub enum FileMode {
    #[allow(unused)]
    Read,
    Write,
}

impl FileMode {
    pub fn as_cstr(&self) -> &'static CStr {
        match self {
            Self::Read => c"rb",
            Self::Write => c"wb",
        }
    }
}

impl File {
    /// Opens a file and checks whether it is prepended by /usb/
    pub fn open(path: &CStr, mode: FileMode) -> Result<Self, ()> {
        if !path.to_bytes().starts_with(b"/usb/") {
            log_error!("Path must start with /usb/");
            return Err(());
        }
        let res = unsafe { fopen(path.as_ptr(), mode.as_cstr().as_ptr()) };
        if res.is_null() {
            log_error!("File Open: NULL ptr");
            Err(())
        } else {
            Ok(Self { fp: res })
        }
    }

    #[allow(unused)]
    pub fn read(&self, buffer: &mut [u8]) -> usize {
        unsafe { fread(buffer.as_mut_ptr(), 1, buffer.len(), self.fp) }
    }

    pub fn write(&self, buf: &[u8]) -> usize {
        unsafe { fwrite(buf.as_ptr(), 1, buf.len(), self.fp) }
    }

    pub fn close(self) {}
}

impl Drop for File {
    fn drop(&mut self) {
        unsafe { fclose(self.fp) };
    }
}

#[allow(unused)]
pub fn test_file() {
    if let Ok(f) = File::open(c"/usb/test.txt", FileMode::Write) {
        log_info!("Test File Opened");
        let written = f.write(b"Hello World\n");
        log_info!("Bytes written: {written}");
        f.close();
        log_info!("Test File Closed");
    }
}
