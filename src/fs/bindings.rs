use core::ffi::c_char;

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
