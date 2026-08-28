use core::marker::PhantomData;

use alloc::slice;

use super::bindings::*;

pub struct ZeroCopyPacketBuffer {
    inner: *mut lwip_pbuf,
    current: *mut lwip_pbuf,
}

pub struct ZeroCopyPacketBufferIterator<'a> {
    inner: ZeroCopyPacketBuffer,
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
    pub fn as_mut_ptr(&self) -> *mut lwip_pbuf {
        self.inner
    }

    pub fn total_len(&self) -> u16 {
        unsafe { (*self.inner).tot_len }
    }

    pub fn iter<'a>(self) -> ZeroCopyPacketBufferIterator<'a> {
        let current = self.current;
        ZeroCopyPacketBufferIterator {
            inner: self,
            current,
            _phantom: PhantomData,
        }
    }
}

impl Drop for ZeroCopyPacketBuffer {
    fn drop(&mut self) {
        unsafe { pbuf_free(self.inner) };
    }
}

/*
impl<'a> IntoIterator for ZeroCopyPacketBuffer {
    type Item = &'a [u8];
    type IntoIter = ZeroCopyPacketBufferIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        ZeroCopyPacketBufferIterator {
            inner: self,
            current: self.current,
            _phantom: PhantomData,
        }
    }
}
*/

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

/*

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

    pub fn into_tcp_packet(self) -> TcpPacket {
        let mut data = [0u8; 1024];
        let copied = unsafe {
            pbuf_copy_partial(
                self.inner as *const lwip_pbuf,
                data.as_mut_ptr(),
                data.len() as u16,
                0,
            )
        };
        TcpPacket {
            arr: data,
            len: copied as usize,
        }
    }

    pub fn into_udp_packet(self) -> UdpPacket {
        let mut data = [0u8; 1024];
        let copied = unsafe {
            pbuf_copy_partial(
                self.inner as *const lwip_pbuf,
                data.as_mut_ptr(),
                data.len() as u16,
                0,
            )
        };
        UdpPacket {
            arr: data,
            len: copied as usize,
        }
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

pub struct TcpPacket {
    arr: [u8; 1024],
    len: usize,
}

impl TcpPacket {
    pub fn as_bytes(&self) -> &[u8] {
        &self.arr[..self.len]
    }
}

impl Drop for TcpPacket {
    fn drop(&mut self) {}
}

pub struct UdpPacket {
    arr: [u8; 1024],
    len: usize,
}

impl UdpPacket {
    pub fn as_bytes(&self) -> &[u8] {
        &self.arr[..self.len]
    }
}
*/
