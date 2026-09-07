use core::ffi::c_void;

use alloc::boxed::Box;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};

use crate::{
    log_error,
    lwip::{bindings::*, packet_buffer::PacketBuffer},
};

pub trait Mode {}

pub struct InThread;
impl Mode for InThread {}

pub struct OutThread;
impl Mode for OutThread {}

pub struct TcpProtocolControlBlock<T: Mode> {
    inner: *mut lwip_pcb,
    _mode: T,
}

impl TryFrom<*mut lwip_pcb> for TcpProtocolControlBlock<InThread> {
    type Error = ();
    fn try_from(value: *mut lwip_pcb) -> Result<Self, ()> {
        if value.is_null() {
            Err(())
        } else {
            Ok(Self {
                inner: value,
                _mode: InThread,
            })
        }
    }
}

impl TryFrom<*mut lwip_pcb> for TcpProtocolControlBlock<OutThread> {
    type Error = ();
    fn try_from(value: *mut lwip_pcb) -> Result<Self, ()> {
        if value.is_null() {
            Err(())
        } else {
            Ok(Self {
                inner: value,
                _mode: OutThread,
            })
        }
    }
}

impl From<TcpProtocolControlBlock<InThread>> for TcpProtocolControlBlock<OutThread> {
    fn from(value: TcpProtocolControlBlock<InThread>) -> Self {
        TcpProtocolControlBlock {
            inner: value.inner,
            _mode: OutThread,
        }
    }
}

impl<T: Mode> TcpProtocolControlBlock<T> {
    pub fn as_mut_ptr(&self) -> *mut lwip_pcb {
        self.inner
    }
}

impl TcpProtocolControlBlock<InThread> {
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

impl TcpProtocolControlBlock<OutThread> {
    pub fn new() -> Result<Self, ()> {
        unsafe { sys_mutex_lock(&raw mut lock_tcpip_core) };
        let pcb = unsafe { tcp_new() };
        unsafe { sys_mutex_unlock(&raw mut lock_tcpip_core) };
        Self::try_from(pcb)
    }

    pub fn with_core<R>(
        &mut self,
        fcn: impl FnOnce(&mut TcpProtocolControlBlock<InThread>) -> R,
    ) -> R {
        let pcb = unsafe { &mut *(self as *mut Self as *mut TcpProtocolControlBlock<InThread>) };
        unsafe { sys_mutex_lock(&raw mut lock_tcpip_core) };
        let res = fcn(pcb);
        unsafe { sys_mutex_unlock(&raw mut lock_tcpip_core) };
        res
    }
}

pub type TcpHandler = embassy_sync::channel::Channel<ThreadModeRawMutex, Box<TcpHandle>, 3>;

pub struct TcpHandle {
    pub packets: embassy_sync::channel::Channel<ThreadModeRawMutex, Option<PacketBuffer>, 10>,
    pub pcb: TcpProtocolControlBlock<OutThread>,
    closed: bool,
}

impl TcpHandle {
    pub fn new(pcb: TcpProtocolControlBlock<InThread>) -> Self {
        let pcb = TcpProtocolControlBlock {
            inner: pcb.inner,
            _mode: OutThread,
        };
        Self {
            packets: Channel::new(),
            pcb,
            closed: false,
        }
    }

    pub fn close(&mut self) {
        self.pcb.with_core(|pcb| {
            pcb.recv(None);
            pcb.err(None);
            pcb.sent(None);
            pcb.accept(None);
            if let Err(e) = pcb.output() {
                log_error!("handler.close(): {e:?}");
            }
            if let Err(e) = pcb.close() {
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
            self.pcb.with_core(|pcb| {
                if let Err(e) = pcb.write(bytes) {
                    log_error!("handler.send_response(): {e:?}");
                }
            });
            self.close();
        }
    }

    pub unsafe fn recv_arg(&mut self, arg: *mut c_void) {
        unsafe { tcp_arg(self.pcb.inner, arg) };
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
