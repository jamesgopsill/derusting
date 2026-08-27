use alloc::vec::Vec;

use super::bindings::*;

pub struct PacketBuffer {
    inner: *mut lwip_pbuf,
}

impl TryFrom<*mut lwip_pbuf> for PacketBuffer {
    type Error = ();
    fn try_from(value: *mut lwip_pbuf) -> Result<Self, ()> {
        if value.is_null() {
            Err(())
        } else {
            Ok(Self { inner: value })
        }
    }
}

impl PacketBuffer {
    pub fn alloc(size: u16) -> Option<Self> {
        let pbuf = unsafe { pbuf_alloc(PbufLayer::Transport, size, PbufType::Ram) };
        match pbuf.is_null() {
            true => None,
            false => Some(Self { inner: pbuf }),
        }
    }

    pub fn as_mut_ptr(&self) -> *mut lwip_pbuf {
        self.inner
    }

    pub fn total_len(&self) -> u16 {
        unsafe { (*self.inner).tot_len }
    }

    pub fn as_vec(&self) -> Vec<u8> {
        let mut data: Vec<u8> = Vec::with_capacity(self.total_len() as usize);
        unsafe {
            let copied = pbuf_copy_partial(
                self.inner as *const lwip_pbuf,
                data.as_mut_ptr(),
                data.capacity() as u16,
                0,
            );
            data.set_len(copied as usize);
        };
        data
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.as_vec()
    }

    pub fn as_array(&self) -> (usize, [u8; 1024]) {
        let mut data = [0u8; 1024];
        let copied = unsafe {
            pbuf_copy_partial(
                self.inner as *const lwip_pbuf,
                data.as_mut_ptr(),
                data.len() as u16,
                0,
            )
        };
        (copied as usize, data)
    }

    pub fn write(&self, data: &[u8]) {
        let pbuf = unsafe { &mut *self.inner };
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), pbuf.payload, data.len());
        }
    }
}

impl Drop for PacketBuffer {
    fn drop(&mut self) {
        unsafe { pbuf_free(self.inner) };
    }
}
