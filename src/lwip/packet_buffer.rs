use core::marker::PhantomData;

use alloc::slice;

use super::bindings::*;

pub struct ZeroCopyPacketBuffer {
    inner: *mut lwip_pbuf,
    current: *mut lwip_pbuf,
}

pub struct ZeroCopyPacketBufferIterator<'a> {
    // Must hold onto the original so it can be dropped
    // once the iterator is done to free the underlying
    // pbuf.
    _inner: ZeroCopyPacketBuffer,
    current: *mut lwip_pbuf,
    _phantom: PhantomData<&'a [u8]>,
}

unsafe impl Send for ZeroCopyPacketBuffer {}
unsafe impl Sync for ZeroCopyPacketBuffer {}

impl TryFrom<*mut lwip_pbuf> for ZeroCopyPacketBuffer {
    type Error = ();
    fn try_from(value: *mut lwip_pbuf) -> Result<Self, ()> {
        if value.is_null() {
            Err(())
        } else {
            Ok(Self {
                inner: value,
                current: value,
            })
        }
    }
}

impl ZeroCopyPacketBuffer {
    pub fn alloc() -> Option<Self> {
        let pbuf = unsafe { pbuf_alloc(PbufLayer::Transport, 1024, PbufType::Ram) };
        match pbuf.is_null() {
            true => None,
            false => Some(Self {
                inner: pbuf,
                current: pbuf,
            }),
        }
    }

    pub fn total_len(&self) -> u16 {
        unsafe { (*self.inner).tot_len }
    }

    pub fn iter<'a>(self) -> ZeroCopyPacketBufferIterator<'a> {
        let current = self.current;
        ZeroCopyPacketBufferIterator {
            _inner: self,
            current,
            _phantom: PhantomData,
        }
    }

    pub fn as_mut_ptr(&mut self) -> *mut lwip_pbuf {
        self.inner
    }

    pub fn with_payload(&mut self, fcn: impl FnOnce(&mut [u8]) -> bool) -> bool {
        let payload_ptr = unsafe { (*self.inner).payload };
        let payload = unsafe { core::slice::from_raw_parts_mut(payload_ptr, 1024) };
        fcn(payload)
    }
}

impl Drop for ZeroCopyPacketBuffer {
    fn drop(&mut self) {
        unsafe { pbuf_free(self.inner) };
    }
}

impl<'a> Iterator for ZeroCopyPacketBufferIterator<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        if self.current.is_null() {
            return None;
        }
        unsafe {
            let p = &*self.current;
            let slice = slice::from_raw_parts(p.payload as *const u8, p.len as usize);
            self.current = p.next;
            Some(slice)
        }
    }
}
