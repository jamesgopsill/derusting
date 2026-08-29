use core::ffi::c_void;

use crate::{
    log_error, log_info,
    lwip::{packet_buffer::ZeroCopyPacketBuffer, tcp::InThreadTcpProtocolControlBlock},
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

    pcb.arg(arg);
    pcb.recv(Some(on_tcp_recv));
    pcb.err(Some(on_tcp_err));
    pcb.sent(Some(on_tcp_sent));

    let tcp_socket = unsafe { &*(arg as *const super::TcpSocket) };
    tcp_socket.on_accept_add_pcb(pcb);

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
    let sock = unsafe { &*(arg as *const super::TcpSocket) };

    let Ok(pcb) = InThreadTcpProtocolControlBlock::try_from(pcb) else {
        log_error!("pcb is null");
        return LwipError::Ok;
    };

    let Ok(pbuf) = ZeroCopyPacketBuffer::try_from(pbuf) else {
        log_info!("Remote host closed connection");
        // NOTE: I think the channel size is larger than the
        // number of concurrent pbufs so we should always succeed.
        let _ = sock.packets.try_send(None);
        return LwipError::Ok;
    };

    let len = pbuf.total_len();
    if sock.packets.try_send(Some(pbuf)).is_ok() {
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
        let sock = unsafe { &*(arg as *const super::TcpSocket) };
        let _ = sock.packets.try_send(None);
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
