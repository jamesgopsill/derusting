use core::{ffi::c_void, ptr};

use alloc::boxed::Box;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};

use crate::{
    TCP_CHANNELS,
    free_rtos::executor::__pender,
    http::INTERNAL_SERVER_ERROR,
    log_error, log_info,
    lwip::{handler::Handler, packet_buffer::PacketBuffer, tcp::TcpProtocolControlBlock},
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

    let Ok(tcp) = TcpProtocolControlBlock::try_from(pcb) else {
        log_error!("pcb is null");
        return LwipError::Ok;
    };

    if arg.is_null() {
        return LwipError::Ok;
    }
    let channels = unsafe { &*(arg as *const crate::TcpChannels) };

    let h = Handler::new(tcp.clone());
    let h = Box::new(h);

    let raw_ptr = &*h as *const Handler as *mut c_void;

    // Assign the handler to the connection.
    tcp.arg(raw_ptr);
    tcp.recv(Some(on_tcp_recv));
    tcp.err(Some(on_tcp_err));

    if channels.try_send(h).is_err() {
        tcp.arg(core::ptr::null_mut());
    }

    LwipError::Ok
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_tcp_recv(
    arg: *mut c_void,
    pcb: *mut lwip_pcb,
    pbuf: *mut lwip_pbuf,
    err: LwipError,
) -> LwipError {
    if arg.is_null() {
        return LwipError::Ok;
    }

    // Note. unsure of dropping handler here.
    // Should it be the async runtime when it hits a timeout.

    let ch = unsafe { &mut *(arg as *mut Handler) };

    let Ok(tcp) = TcpProtocolControlBlock::try_from(pcb) else {
        let _to_drop = unsafe { Box::from_raw(arg as *mut Handler) };
        return LwipError::Ok;
    };

    let Ok(pbuf) = PacketBuffer::try_from(pbuf) else {
        let h = unsafe { Box::from_raw(arg as *mut Handler) };
        h.write_and_close(INTERNAL_SERVER_ERROR.as_bytes());
        return LwipError::Ok;
    };

    if err != LwipError::Ok {
        let h = unsafe { Box::from_raw(arg as *mut Handler) };
        h.write_and_close(INTERNAL_SERVER_ERROR.as_bytes());
        return LwipError::Ok;
    }

    // TODO: Allow to fail
    let v = pbuf.as_array();
    loop {
        if ch.channel.try_send(v).is_ok() {
            break;
        }
        log_info!("Failed to send to channel -- waiting");
        unsafe { sys_msleep(50) };
    }
    tcp.recved(pbuf.total_len());
    /*
    if let Err(e) = ch.channel.try_send(pbuf.as_vec()) {
        log_error!("Could not send over channel");
        //let h = unsafe { Box::from_raw(arg as *mut Handler) };
        //h.write_and_close(INTERNAL_SERVER_ERROR.as_bytes());
    };
    */

    LwipError::Ok
}

#[unsafe(no_mangle)]
unsafe extern "C" fn on_tcp_err(arg: *mut c_void, _err: LwipError) {
    log_info!("ERROR hit");
}
