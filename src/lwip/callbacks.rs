use core::ffi::c_void;

use crate::{
    log_error, log_info,
    lwip::{packet_buffer::PacketBuffer, tcp::TcpProtocolControlBlock},
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
    log_info!("on_tcp_accept()");
    if err != LwipError::Ok {
        log_error!("TCPError");
        return LwipError::Ok;
    }

    let Ok(pcb) = TcpProtocolControlBlock::try_from(pcb) else {
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
    _pcb: *mut lwip_pcb, // Should get the right one from the arg as we're only dealing with single requests.
    pbuf: *mut lwip_pbuf,
    _err: LwipError,
) -> LwipError {
    log_info!("on_tcp_recv");
    if arg.is_null() {
        return LwipError::Ok;
    }
    let sock = unsafe { &*(arg as *const super::TcpSocket) };

    let Ok(pbuf) = PacketBuffer::try_from(pbuf) else {
        log_info!("Remote host closed connection");
        let _ = sock.close();
        return LwipError::Ok;
    };

    let len = pbuf.total_len();
    let packet = pbuf.into_tcp_packet();
    if sock.packets.try_send(Some(packet)).is_ok() {
        log_info!("Sent");
        sock.recevd_in_lwip_thread(len);
        LwipError::Ok
    } else {
        LwipError::Mem
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn on_tcp_err(arg: *mut c_void, _err: LwipError) {
    if !arg.is_null() {
        let sock = unsafe { &*(arg as *const super::TcpSocket) };
        let _ = sock.close();
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
