use core::ffi::c_void;

use crate::lwip::{bindings::*, core::LwipCore};

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
