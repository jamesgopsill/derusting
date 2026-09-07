use core::marker::PhantomData;

use alloc::slice;
use postcard::ser_flavors::Size;
use serde::Serialize;

use crate::log_error;

use super::bindings::*;

/// A zero copy wapper around a LWIP PBUF.
pub struct PacketBuffer {
    inner: *mut lwip_pbuf,
    current: *mut lwip_pbuf,
}

/// An iterator over a pbuf chain to read and process data from.
pub struct PacketBufferIterator<'a> {
    // Must hold onto the original so it can be dropped
    // once the iterator is done to free the underlying
    // pbuf.
    _inner: PacketBuffer,
    current: *mut lwip_pbuf,
    _phantom: PhantomData<&'a [u8]>,
}

unsafe impl Send for PacketBuffer {}
unsafe impl Sync for PacketBuffer {}

impl TryFrom<*mut lwip_pbuf> for PacketBuffer {
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

impl PacketBuffer {
    /// Allocate a PacketBuffer from lwip.
    pub fn alloc<T: Serialize>(msg: T) -> Option<Self> {
        let Ok(size) = postcard::serialize_with_flavor(&msg, Size::default()) else {
            log_error!("serialize_with_flavor error");
            return None;
        };
        let pbuf = unsafe { pbuf_alloc(PbufLayer::Transport, size as u16, PbufType::Ram) };
        if pbuf.is_null() {
            log_error!("NULL pbuf");
            return None;
        }
        let payload_ptr = unsafe { (*pbuf).payload };
        let payload = unsafe { core::slice::from_raw_parts_mut(payload_ptr, size) };
        if let Err(_) = postcard::to_slice(&msg, payload) {
            log_error!("Serialization error");
            unsafe { pbuf_free(pbuf) };
            return None;
        }
        Some(Self {
            inner: pbuf,
            current: pbuf,
        })
    }

    /// Get the total length of data held within a packet buffer chain.
    pub fn total_len(&self) -> u16 {
        unsafe { (*self.inner).tot_len }
    }

    /// Turns a packet buffer into an iterator over the data it holds
    /// within its chain.
    pub fn into_iter<'a>(self) -> PacketBufferIterator<'a> {
        let current = self.current;
        PacketBufferIterator {
            _inner: self,
            current,
            _phantom: PhantomData,
        }
    }

    /// Returns the `*mut lwip_pbuf`
    pub fn as_mut_ptr(&mut self) -> *mut lwip_pbuf {
        self.inner
    }
}

impl Drop for PacketBuffer {
    fn drop(&mut self) {
        // Free the packet buffer so LWIP can allocate it again.
        unsafe { pbuf_free(self.inner) };
    }
}

impl<'a> Iterator for PacketBufferIterator<'a> {
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
