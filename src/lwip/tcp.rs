use core::{default, ffi::c_void, ptr, sync::atomic::Ordering};

use alloc::{boxed::Box, vec::Vec};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Channel, TrySendError},
};
use portable_atomic::AtomicPtr;

use crate::lwip::{self, bindings::*, core::LwipCore};

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

    pub fn err(&self, callback: Option<LwipErrFn>) {
        unsafe { tcp_err(self.as_mut_ptr(), callback) };
    }

    pub fn close_with_core(self, _core: &LwipCore) -> Result<(), LwipError> {
        self.close()
    }

    pub fn close(self) -> Result<(), LwipError> {
        let err = unsafe { tcp_close(self.as_mut_ptr()) };
        err.into()
    }

    pub fn recved(&self, len: u16) {
        unsafe { tcp_recved(self.as_mut_ptr(), len) };
    }

    // TODO: add error parsing
    pub fn write_with_core(&self, slice: &[u8], _core: &LwipCore) {
        self.write(slice);
    }

    pub fn write(&self, slice: &[u8]) -> Result<(), LwipError> {
        // TODO: check the u16 conversion (suppose it will just clip the data)
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

    pub fn accept(&self, callback: Option<TcpAcceptFn>, core: &LwipCore) {
        unsafe { tcp_accept(self.as_mut_ptr(), callback) };
    }

    pub fn listen_with_backlog(
        &self,
        backlog: u8,
        core: &LwipCore,
    ) -> Result<TcpProtocolControlBlock, ()> {
        let pcb = unsafe { tcp_listen_with_backlog(self.as_mut_ptr(), backlog) };
        TcpProtocolControlBlock::try_from(pcb)
    }
}
