use core::{default, ffi::c_void, ptr, sync::atomic::Ordering};

use alloc::{boxed::Box, vec::Vec};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Channel, TrySendError},
};
use portable_atomic::AtomicPtr;

use crate::{
    log_error, log_info,
    lwip::{bindings::*, core::LwipCore, packet_buffer::PacketBuffer, pico::PicoSocket},
};

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

pub struct TcpChannel {
    inner: Channel<CriticalSectionRawMutex, Vec<u8>, 1>,
    tcp: TcpProtocolControlBlock,
}

impl TcpChannel {
    fn new(tcp: TcpProtocolControlBlock) -> Self {
        Self {
            inner: Channel::new(),
            tcp,
        }
    }

    pub fn write(&self, bytes: &[u8]) -> Result<(), LwipError> {
        self.tcp.write(bytes)
    }
}

impl TcpChannel {
    pub fn try_send(&self, data: Vec<u8>) -> Result<(), TrySendError<Vec<u8>>> {
        self.inner.try_send(data)
    }

    pub async fn receive(&self) -> Vec<u8> {
        self.inner.receive().await
    }
}

impl Drop for TcpChannel {
    fn drop(&mut self) {
        self.tcp.recv(None);
        self.tcp.err(None);
        self.tcp.arg(ptr::null_mut());
    }
}

// NOTE: do not need to hold the lock as this occurs
// in the TCP/IP thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_tcp_accept(
    _arg: *mut c_void,
    pcb: *mut lwip_pcb,
    err: LwipError,
) -> LwipError {
    log_info!("on_accept()");
    if err != LwipError::Ok {
        log_error!("TCPError");
        return LwipError::Ok;
    }

    let Ok(tcp) = TcpProtocolControlBlock::try_from(pcb) else {
        log_error!("pcb is null");
        return LwipError::Ok;
    };

    let socket: PicoSocket<picoserve::EmbassyRuntime> = PicoSocket::new(tcp.clone());
    let ch = Box::new(socket);
    let raw_ptr = Box::into_raw(ch) as *mut c_void;

    // Assign the handler to the connection.
    tcp.arg(raw_ptr);
    tcp.recv(Some(on_tcp_recv));
    tcp.err(Some(on_tcp_err));

    // TODO: poll
    LwipError::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_tcp_recv(
    arg: *mut c_void,
    pcb: *mut lwip_pcb,
    pbuf: *mut lwip_pbuf,
    err: LwipError,
) -> LwipError {
    log_info!("on_recv()");

    if arg.is_null() {
        return LwipError::Ok;
    }

    let channel = unsafe { &mut *(arg as *mut TcpChannel) };

    let Ok(tcp) = TcpProtocolControlBlock::try_from(pcb) else {
        let _to_drop = unsafe { Box::from_raw(arg as *mut TcpChannel) };
        return LwipError::Ok;
    };

    let Ok(pbuf) = PacketBuffer::try_from(pbuf) else {
        tcp.recv(None);
        tcp.err(None);
        tcp.arg(ptr::null_mut());
        let _ = tcp.close();
        let _to_drop = unsafe { Box::from_raw(arg as *mut TcpChannel) };
        return LwipError::Ok;
    };

    if err != LwipError::Ok {
        tcp.recv(None);
        tcp.err(None);
        tcp.arg(ptr::null_mut());
        let _ = tcp.close();
        let _to_drop = unsafe { Box::from_raw(arg as *mut TcpChannel) };
        return LwipError::Ok;
    }

    tcp.recved(pbuf.total_len());

    if let Err(e) = channel.try_send(pbuf.as_vec()) {
        log_error!("Could not send over channel");
        tcp.write(INTERNAL_SERVER_ERROR.as_bytes());
    };

    LwipError::Ok
}

#[unsafe(no_mangle)]
unsafe extern "C" fn on_tcp_err(arg: *mut c_void, _err: LwipError) {
    log_info!("ERROR hit");
}

pub const INTERNAL_SERVER_ERROR: &str =
    "HTTP/1.1 500 Internal Server Error\r\nContent-length:0\r\nConnection: close\r\n\r\n";
/*
// NOTE: do not need to hold the lock as this occurs
// in the TCP/IP thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_tcp_accept(
    _arg: *mut c_void,
    pcb: *mut lwip_pcb,
    err: LwipError,
) -> LwipError {
    log_info!("on_accept()");
    if err != LwipError::Ok {
        log_error!("TCPError");
        return LwipError::Ok;
    }

    let Ok(tcp) = TcpProtocolControlBlock::try_from(pcb) else {
        log_error!("pcb is null");
        return LwipError::Ok;
    };

    let ch = Box::new(TcpChannel::new(tcp.clone()));
    let raw_ptr = Box::into_raw(ch) as *mut c_void;

    // Assign the handler to the connection.
    tcp.arg(raw_ptr);
    tcp.recv(Some(on_tcp_recv));
    tcp.err(Some(on_tcp_err));

    // TODO: poll
    LwipError::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_tcp_recv(
    arg: *mut c_void,
    pcb: *mut lwip_pcb,
    pbuf: *mut lwip_pbuf,
    err: LwipError,
) -> LwipError {
    log_info!("on_recv()");

    if arg.is_null() {
        return LwipError::Ok;
    }

    let channel = unsafe { &mut *(arg as *mut TcpChannel) };

    let Ok(tcp) = TcpProtocolControlBlock::try_from(pcb) else {
        let _to_drop = unsafe { Box::from_raw(arg as *mut TcpChannel) };
        return LwipError::Ok;
    };

    let Ok(pbuf) = PacketBuffer::try_from(pbuf) else {
        tcp.recv(None);
        tcp.err(None);
        tcp.arg(ptr::null_mut());
        let _ = tcp.close();
        let _to_drop = unsafe { Box::from_raw(arg as *mut TcpChannel) };
        return LwipError::Ok;
    };

    if err != LwipError::Ok {
        tcp.recv(None);
        tcp.err(None);
        tcp.arg(ptr::null_mut());
        let _ = tcp.close();
        let _to_drop = unsafe { Box::from_raw(arg as *mut TcpChannel) };
        return LwipError::Ok;
    }

    tcp.recved(pbuf.total_len());

    if let Err(e) = channel.try_send(pbuf.as_vec()) {
        log_error!("Could not send over channel");
        channel.tcp.write(INTERNAL_SERVER_ERROR.as_bytes());
    };

    LwipError::Ok
}

#[unsafe(no_mangle)]
unsafe extern "C" fn on_tcp_err(arg: *mut c_void, _err: LwipError) {
    log_info!("ERROR hit");
    // TODO: How to drop safely as it will be being used in a task.
    /*
    if !arg.is_null() {
        let _to_drop = unsafe { Box::from_raw(arg as *mut TcpChannel) };
    }
    Will probably need to wrap a recieved with a select the listens for
    data or a signal from here to report and error
    */
    todo!()
}

pub const INTERNAL_SERVER_ERROR: &str =
    "HTTP/1.1 500 Internal Server Error\r\nContent-length:0\r\nConnection: close\r\n\r\n";
*/
