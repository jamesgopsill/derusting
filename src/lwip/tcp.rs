use core::ffi::c_void;

use alloc::boxed::Box;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};

use crate::{
    log_error,
    lwip::{self, bindings::*, core::LwipCore, packet_buffer::ZeroCopyPacketBuffer},
};

#[derive(Debug)]
pub struct InThreadTcpProtocolControlBlock {
    inner: *mut lwip_pcb,
}

#[derive(Debug)]
pub struct OutThreadTcpProtocolControlBlock {
    inner: InThreadTcpProtocolControlBlock,
}

impl TryFrom<*mut lwip_pcb> for InThreadTcpProtocolControlBlock {
    type Error = ();
    fn try_from(value: *mut lwip_pcb) -> Result<Self, ()> {
        if value.is_null() {
            Err(())
        } else {
            Ok(Self { inner: value })
        }
    }
}

impl TryFrom<*mut lwip_pcb> for OutThreadTcpProtocolControlBlock {
    type Error = ();
    fn try_from(value: *mut lwip_pcb) -> Result<Self, ()> {
        if value.is_null() {
            Err(())
        } else {
            let inner = InThreadTcpProtocolControlBlock::try_from(value)?;
            Ok(Self { inner })
        }
    }
}

impl From<InThreadTcpProtocolControlBlock> for OutThreadTcpProtocolControlBlock {
    fn from(value: InThreadTcpProtocolControlBlock) -> Self {
        OutThreadTcpProtocolControlBlock { inner: value }
    }
}

impl OutThreadTcpProtocolControlBlock {
    pub fn new(_core: &LwipCore) -> Result<Self, ()> {
        let pcb = unsafe { tcp_new() };
        Self::try_from(pcb)
    }

    pub fn as_mut_ptr(&self) -> *mut lwip_pcb {
        self.inner.as_mut_ptr()
    }

    pub fn arg(&self, arg: *mut c_void, _core: &LwipCore) {
        self.inner.arg(arg)
    }

    pub fn recv(&self, callback: Option<TcpRecvFn>, _core: &LwipCore) {
        self.inner.recv(callback)
    }

    pub fn err(&self, callback: Option<LwipErrFn>, _core: &LwipCore) {
        self.inner.err(callback)
    }

    pub fn close(&self, _core: &LwipCore) -> Result<(), LwipError> {
        self.inner.close()
    }

    #[allow(unused)]
    pub fn recved(&self, len: u16, _core: &LwipCore) {
        self.inner.recved(len)
    }

    pub fn write(&self, slice: &[u8], _core: &LwipCore) -> Result<(), LwipError> {
        self.inner.write(slice)
    }

    pub fn output(&self, _core: &LwipCore) -> Result<(), LwipError> {
        self.inner.output()
    }

    pub fn bind(&self, port: u16, _core: &LwipCore) -> Result<(), LwipError> {
        self.inner.bind(port)
    }

    pub fn accept(&self, callback: Option<TcpAcceptFn>, _core: &LwipCore) {
        self.inner.accept(callback)
    }

    pub fn listen_with_backlog(&self, backlog: u8, _core: &LwipCore) -> Result<Self, ()> {
        let inner = self.inner.listen_with_backlog(backlog)?;
        Ok(Self { inner })
    }

    pub fn sent(&self, callback: Option<TcpSentFn>, _core: &LwipCore) {
        self.inner.sent(callback);
    }
}

impl InThreadTcpProtocolControlBlock {
    pub fn as_mut_ptr(&self) -> *mut lwip_pcb {
        self.inner
    }

    pub fn arg(&self, arg: *mut c_void) {
        unsafe { tcp_arg(self.as_mut_ptr(), arg) };
    }

    pub fn recv(&self, callback: Option<TcpRecvFn>) {
        unsafe { tcp_recv(self.as_mut_ptr(), callback) };
    }

    pub fn err(&self, callback: Option<LwipErrFn>) {
        unsafe { tcp_err(self.as_mut_ptr(), callback) };
    }

    pub fn close(&self) -> Result<(), LwipError> {
        let err = unsafe { tcp_close(self.as_mut_ptr()) };
        err.into()
    }

    pub fn recved(&self, len: u16) {
        unsafe { tcp_recved(self.as_mut_ptr(), len) };
    }

    pub fn write(&self, slice: &[u8]) -> Result<(), LwipError> {
        let err = unsafe {
            tcp_write(
                self.as_mut_ptr(),
                slice.as_ptr(),
                slice.len() as u16,
                TCP_WRITE_FLAG_COPY,
            )
        };
        err.into()
    }

    pub fn output(&self) -> Result<(), LwipError> {
        let err = unsafe { tcp_output(self.as_mut_ptr()) };
        err.into()
    }

    pub fn bind(&self, port: u16) -> Result<(), LwipError> {
        let err = unsafe { tcp_bind(self.as_mut_ptr(), &ip_addr_any, port) };
        err.into()
    }

    pub fn accept(&self, callback: Option<TcpAcceptFn>) {
        unsafe { tcp_accept(self.as_mut_ptr(), callback) };
    }

    pub fn listen_with_backlog(&self, backlog: u8) -> Result<Self, ()> {
        let pcb = unsafe { tcp_listen_with_backlog(self.as_mut_ptr(), backlog) };
        Self::try_from(pcb)
    }

    pub fn sent(&self, callback: Option<TcpSentFn>) {
        unsafe {
            tcp_sent(self.as_mut_ptr(), callback);
        }
    }
}

pub type TcpHandler = embassy_sync::channel::Channel<ThreadModeRawMutex, Box<TcpHandle>, 3>;

pub struct TcpHandle {
    pub packets:
        embassy_sync::channel::Channel<ThreadModeRawMutex, Option<ZeroCopyPacketBuffer>, 10>,
    pcb: OutThreadTcpProtocolControlBlock,
    closed: bool,
}

impl TcpHandle {
    pub fn new(pcb: InThreadTcpProtocolControlBlock) -> Self {
        let pcb = OutThreadTcpProtocolControlBlock::from(pcb);
        Self {
            packets: Channel::new(),
            pcb,
            closed: false,
        }
    }

    pub fn recv_arg(&mut self, arg: *mut c_void) {
        self.pcb.inner.arg(arg);
    }

    pub fn close(&mut self) {
        lwip::core::with_lwip_core(|core| {
            self.pcb.recv(None, &core);
            self.pcb.err(None, &core);
            self.pcb.sent(None, &core);
            self.pcb.accept(None, &core);
            if let Err(e) = self.pcb.output(&core) {
                log_error!("handler.close(): {e:?}");
            }
            if let Err(e) = self.pcb.close(&core) {
                log_error!("handler.close(): {e:?}");
            }
        });
        self.packets.clear();
        self.closed = true;
    }

    // Out of thread function
    pub fn respond(&mut self, bytes: &[u8]) {
        // Can only send once. Ignore repeated calls.
        if !self.closed {
            lwip::core::with_lwip_core(|core| {
                if let Err(e) = self.pcb.write(bytes, &core) {
                    log_error!("handler.send_response(): {e:?}");
                }
            });
            self.close();
        }
    }
}

impl Drop for TcpHandle {
    // If we haven't closed the handle then close it before
    // we drop it.
    fn drop(&mut self) {
        if !self.closed {
            self.close();
        }
    }
}
