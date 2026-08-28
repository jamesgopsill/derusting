use core::{ffi::c_void, sync::atomic::Ordering};

use portable_atomic::AtomicPtr;

use crate::lwip::{bindings::*, core::LwipCore};

#[derive(Debug, Clone)]
pub struct TcpProtocolControlBlock {
    inner: *mut lwip_pcb,
}

impl TryFrom<*mut lwip_pcb> for TcpProtocolControlBlock {
    type Error = ();
    fn try_from(value: *mut lwip_pcb) -> Result<Self, ()> {
        if value.is_null() {
            Err(())
        } else {
            Ok(Self { inner: value })
        }
    }
}

impl TryFrom<&AtomicPtr<lwip_pcb>> for TcpProtocolControlBlock {
    type Error = ();
    fn try_from(value: &AtomicPtr<lwip_pcb>) -> Result<Self, ()> {
        let value = value.load(Ordering::SeqCst);
        Self::try_from(value)
    }
}

impl TcpProtocolControlBlock {
    pub fn new(_core: &LwipCore) -> Result<Self, ()> {
        let pcb = unsafe { tcp_new() };
        TcpProtocolControlBlock::try_from(pcb)
    }

    pub fn as_mut_ptr(&self) -> *mut lwip_pcb {
        self.inner
    }

    pub fn arg_with_core(&self, arg: *mut c_void, _core: &LwipCore) {
        self.arg(arg)
    }

    pub fn arg(&self, arg: *mut c_void) {
        unsafe { tcp_arg(self.as_mut_ptr(), arg) };
    }

    pub fn recv(&self, callback: Option<TcpRecvFn>) {
        unsafe { tcp_recv(self.as_mut_ptr(), callback) };
    }

    pub fn recv_with_core(&self, callback: Option<TcpRecvFn>, _core: &LwipCore) {
        self.recv(callback);
    }

    pub fn err(&self, callback: Option<LwipErrFn>) {
        unsafe { tcp_err(self.as_mut_ptr(), callback) };
    }

    pub fn err_with_core(&self, callback: Option<LwipErrFn>, _core: &LwipCore) {
        self.err(callback);
    }

    pub fn close_with_core(&self, _core: &LwipCore) -> Result<(), LwipError> {
        self.close()
    }

    pub fn close(&self) -> Result<(), LwipError> {
        let err = unsafe { tcp_close(self.as_mut_ptr()) };
        err.into()
    }

    pub fn recved(&self, len: u16) {
        unsafe { tcp_recved(self.as_mut_ptr(), len) };
    }

    pub fn write_with_core(&self, slice: &[u8], _core: &LwipCore) -> Result<(), LwipError> {
        self.write(slice)
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

    pub fn output_with_core(&self, _core: &LwipCore) -> Result<(), LwipError> {
        self.output()
    }

    pub fn bind(&self, port: u16) -> Result<(), LwipError> {
        let err = unsafe { tcp_bind(self.as_mut_ptr(), &ip_addr_any, port) };
        err.into()
    }

    pub fn bind_with_core(&self, port: u16, _core: &LwipCore) -> Result<(), LwipError> {
        self.bind(port)
    }

    pub fn accept(&self, callback: Option<TcpAcceptFn>) {
        unsafe { tcp_accept(self.as_mut_ptr(), callback) };
    }

    pub fn accept_with_core(&self, callback: Option<TcpAcceptFn>, _core: &LwipCore) {
        self.accept(callback);
    }

    pub fn listen_with_backlog(&self, backlog: u8) -> Result<TcpProtocolControlBlock, ()> {
        let pcb = unsafe { tcp_listen_with_backlog(self.as_mut_ptr(), backlog) };
        TcpProtocolControlBlock::try_from(pcb)
    }

    pub fn listen_with_backlog_with_core(
        &self,
        backlog: u8,
        _core: &LwipCore,
    ) -> Result<TcpProtocolControlBlock, ()> {
        self.listen_with_backlog(backlog)
    }

    pub fn sent(&self, callback: Option<TcpSentFn>) {
        unsafe {
            tcp_sent(self.as_mut_ptr(), callback);
        }
    }

    pub fn sent_with_core(&self, callback: Option<TcpSentFn>, _core: &LwipCore) {
        self.sent(callback);
    }
}
