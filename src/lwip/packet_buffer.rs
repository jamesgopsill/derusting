use core::marker::PhantomData;

use alloc::slice;
use postcard::ser_flavors::Size;
use serde::Serialize;

use crate::{log_error, lwip::blocking_lwip};

use super::bindings::*;

/// A zero copy wapper around a LWIP PBUF.
pub struct PacketBuffer {
    inner: *mut pbuf,
    current: *mut pbuf,
}

/// An iterator over a pbuf chain to read and process data from.
pub struct PacketBufferIterator<'a> {
    // Must hold onto the original so it can be dropped
    // once the iterator is done to free the underlying
    // pbuf.
    _inner: PacketBuffer,
    current: *mut pbuf,
    _phantom: PhantomData<&'a [u8]>,
}

// SAFETY: the only lwIP-mutating operations on a `PacketBuffer` are
// `pbuf_alloc`/`pbuf_free`, both of which now always run with
// `lock_tcpip_core` held (see `alloc` and `Drop` above), so moving/sharing
// this across threads doesn't race lwIP's internal pbuf pool.
unsafe impl Send for PacketBuffer {}
unsafe impl Sync for PacketBuffer {}

impl TryFrom<*mut pbuf> for PacketBuffer {
    type Error = ();
    fn try_from(value: *mut pbuf) -> Result<Self, ()> {
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
        // SAFETY: `pbuf_alloc` has no pointer preconditions; it either
        // returns null (checked below) or a freshly allocated pbuf we take
        // ownership of. lwIP's pbuf allocation must run with
        // `lock_tcpip_core` held, which `blocking_lwip` guarantees.
        let pbuf = blocking_lwip(|| unsafe {
            let pbuf = pbuf_alloc(pbuf_layer::Transport, size as u16, pbuf_type::Ram);
            Ok(pbuf)
        })
        .unwrap();
        if pbuf.is_null() {
            log_error!("NULL pbuf");
            return None;
        }
        // SAFETY: `pbuf` was just allocated and is non-null, so it's a live
        // pbuf we exclusively own at this point.
        let payload_ptr = unsafe { (*pbuf).payload };
        // SAFETY: `pbuf_alloc` was asked for exactly `size` bytes at the
        // `Transport` layer, so `payload_ptr` points to a contiguous,
        // writable buffer of at least `size` bytes that nothing else
        // accesses while we hold `pbuf`.
        let payload = unsafe { core::slice::from_raw_parts_mut(payload_ptr, size) };
        if postcard::to_slice(&msg, payload).is_err() {
            log_error!("Serialization error");
            // SAFETY: `pbuf` is the same non-null pointer allocated above,
            // freed at most once here (function returns immediately after).
            let _ = blocking_lwip(|| unsafe { Ok(pbuf_free(pbuf)) });
            return None;
        }
        Some(Self {
            inner: pbuf,
            current: pbuf,
        })
    }

    /// Get the total length of data held within a packet buffer chain.
    pub fn total_len(&self) -> u16 {
        // SAFETY: `self.inner` is non-null (enforced by `TryFrom`) and
        // remains a live pbuf for the lifetime of `self` (freed only in
        // `Drop`).
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
    pub fn as_mut_ptr(&mut self) -> *mut pbuf {
        self.inner
    }
}

impl Drop for PacketBuffer {
    fn drop(&mut self) {
        // SAFETY: `self.inner` is non-null and owned by this `PacketBuffer`
        // (never shared, never freed elsewhere); `blocking_lwip` ensures
        // `pbuf_free` runs with `lock_tcpip_core` held as lwIP requires.
        let _ = super::blocking_lwip(|| unsafe { Ok(pbuf_free(self.inner)) });
    }
}

impl<'a> Iterator for PacketBufferIterator<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        if self.current.is_null() {
            return None;
        }
        // SAFETY: `self.current` is non-null and part of the pbuf chain
        // rooted at `self._inner.inner`, which stays alive for at least
        // `'a` because `_inner` (the owning `PacketBuffer`) is held inside
        // this iterator; `p.payload`/`p.len` describe a valid, initialised
        // byte range within that still-live pbuf.
        unsafe {
            let p = &*self.current;
            let slice = slice::from_raw_parts(p.payload as *const u8, p.len as usize);
            self.current = p.next;
            Some(slice)
        }
    }
}
