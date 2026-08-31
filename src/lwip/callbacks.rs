use core::ffi::c_void;

use alloc::boxed::Box;

use crate::{
    log_error, log_info,
    lwip::{
        packet_buffer::ZeroCopyPacketBuffer,
        tcp::{InThreadTcpProtocolControlBlock, TcpHandle},
    },
};

use super::bindings::*;

// NOTE: do not need to hold the lock as this occurs
// in the TCP/IP thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_tcp_accept(
    arg: *mut c_void,
    pcb: *mut lwip_pcb,
    err: LwipError,
) -> LwipError {
    log_info!("on_accept");
    if err != LwipError::Ok {
        log_error!("TCPError");
        return LwipError::Ok;
    }

    let Ok(pcb) = InThreadTcpProtocolControlBlock::try_from(pcb) else {
        log_error!("pcb is null");
        return LwipError::Ok;
    };

    if arg.is_null() {
        return LwipError::Ok;
    }

    pcb.recv(Some(on_tcp_recv));
    pcb.err(Some(on_tcp_err));
    pcb.sent(Some(on_tcp_sent));

    // Construct a new TcpHandle. Get a ptr to it and reconstruct
    // it as we need the ptr to pass back to C for it recv fcn and
    // the boxed version is passed to the handle to manage its life
    // on the Rust end.
    let tcp_handle = Box::new(TcpHandle::new(pcb));
    let ptr = Box::into_raw(tcp_handle);
    let mut tcp_handle = unsafe { Box::from_raw(ptr) };
    tcp_handle.recv_arg(ptr as *mut c_void);

    log_info!("Sending handle");
    let tcp_handler = unsafe { &*(arg as *const super::TcpHandler) };
    if tcp_handler.try_send(tcp_handle).is_err() {
        log_error!("TCP Handler channel full");
    };

    LwipError::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_tcp_recv(
    arg: *mut c_void,
    pcb: *mut lwip_pcb,
    pbuf: *mut lwip_pbuf,
    _err: LwipError,
) -> LwipError {
    if arg.is_null() {
        return LwipError::Ok;
    }

    let tcp_handle = unsafe { &*(arg as *const TcpHandle) };

    let Ok(pcb) = InThreadTcpProtocolControlBlock::try_from(pcb) else {
        log_error!("pcb is null");
        return LwipError::Ok;
    };

    let Ok(pbuf) = ZeroCopyPacketBuffer::try_from(pbuf) else {
        log_info!("Remote host closed connection");
        // NOTE: I think the channel size is larger than the
        // number of concurrent pbufs so we should always succeed.
        let _ = tcp_handle.packets.try_send(None);
        return LwipError::Ok;
    };

    let len = pbuf.total_len();
    if tcp_handle.packets.try_send(Some(pbuf)).is_ok() {
        pcb.recved(len);
        LwipError::Ok
    } else {
        log_error!("Channel full");
        LwipError::Mem
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn on_tcp_err(arg: *mut c_void, _err: LwipError) {
    if !arg.is_null() {
        log_error!("on_tcp_err");
        let tcp_handle = unsafe { &*(arg as *const TcpHandle) };
        let _ = tcp_handle.packets.try_send(None);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_tcp_sent(
    _arg: *mut c_void,
    _pcb: *mut lwip_pcb,
    _len: u16,
) -> LwipError {
    LwipError::Ok
}
