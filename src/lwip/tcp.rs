use core::{ffi::c_void, marker::PhantomPinned, pin::Pin};

use embassy_sync::{
    blocking_mutex::raw::ThreadModeRawMutex,
    channel::{Channel, Sender},
};

use crate::{
    log_error, log_info,
    lwip::{bindings::*, packet_buffer::PacketBuffer},
};

pub struct TcpListener<'a, const N: usize, const M: usize> {
    pcb: *mut lwip_pcb,
    sender: Sender<'a, ThreadModeRawMutex, *mut lwip_pcb, N>,
    _pin: PhantomPinned,
}

impl<'a, const N: usize, const M: usize> TcpListener<'a, N, M> {
    pub fn new(sender: Sender<'a, ThreadModeRawMutex, *mut lwip_pcb, N>) -> Self {
        Self {
            pcb: core::ptr::null_mut(),
            sender,
            _pin: PhantomPinned,
        }
    }

    pub fn listen(self: Pin<&mut Self>, port: u16) -> Result<(), ()> {
        unsafe {
            let this = self.get_unchecked_mut();
            sys_mutex_lock(&raw mut lock_tcpip_core);
            this.pcb = tcp_new();
            let err = tcp_bind(this.pcb, &ip_addr_any, port);
            if err != LwipError::Ok {
                tcp_close(this.pcb);
                return Err(());
            }
            this.pcb = tcp_listen_with_backlog(this.pcb, 1);
            if this.pcb.is_null() {
                return Err(());
            }
            let ctx_ptr = this as *mut Self as *mut c_void;
            tcp_arg(this.pcb, ctx_ptr);
            tcp_accept(this.pcb, Some(Self::_accept));
            sys_mutex_unlock(&raw mut lock_tcpip_core);
            Ok(())
        }
    }

    pub unsafe extern "C" fn _accept(
        arg: *mut c_void,
        pcb: *mut lwip_pcb,
        err: LwipError,
    ) -> LwipError {
        log_info!("on_accept");
        if err != LwipError::Ok {
            log_error!("TCPError");
            return LwipError::Ok;
        }

        if pcb.is_null() {
            log_error!("PCB Null");
            return LwipError::Ok;
        }

        if arg.is_null() {
            return LwipError::Ok;
        }

        let listener = unsafe { &*(arg as *const TcpListener<'a, N, M>) };

        if listener.sender.try_send(pcb).is_err() {
            log_error!("TCP Listener Channel Full");
        };

        LwipError::Ok
    }
}

impl<'a, const N: usize, const M: usize> Drop for TcpListener<'a, N, M> {
    fn drop(&mut self) {
        unsafe {
            sys_mutex_lock(&raw mut lock_tcpip_core);
            tcp_accept(self.pcb, None);
            tcp_arg(self.pcb, core::ptr::null_mut());
            let err = tcp_close(self.pcb);
            if err != LwipError::Ok {
                log_error!("tcp_close: {err:?}");
            }
            self.sender.clear();
            sys_mutex_unlock(&raw mut lock_tcpip_core);
        }
    }
}

// TODO: We need to send *mut lwip_buf through the channel
// and pin construct the TcpConn on receipt for the channel
// to prevent it moving on the stack.

pub struct TcpConn<const N: usize> {
    pcb: *mut lwip_pcb,
    channel: Channel<ThreadModeRawMutex, Option<PacketBuffer>, N>,
    _pin: PhantomPinned,
}

impl<const N: usize> TcpConn<N> {
    pub fn new(pcb: *mut lwip_pcb) -> Self {
        Self {
            pcb,
            channel: Channel::new(),
            _pin: PhantomPinned,
        }
    }

    pub fn attach_callbacks(self: Pin<&mut Self>) {
        unsafe {
            let this = self.get_unchecked_mut();
            sys_mutex_lock(&raw mut lock_tcpip_core);
            let ctx_ptr = this as *mut Self as *mut c_void;
            tcp_arg(this.pcb, ctx_ptr);
            tcp_recv(this.pcb, Some(Self::_recv));
            tcp_err(this.pcb, Some(Self::_err));
            tcp_sent(this.pcb, Some(Self::_sent));
            sys_mutex_unlock(&raw mut lock_tcpip_core);
        }
    }

    pub async fn receive(self: Pin<&Self>) -> Option<PacketBuffer> {
        self.channel.receive().await
    }

    pub fn response(self: Pin<&mut Self>, bytes: &[u8]) -> Result<(), LwipError> {
        log_info!("Writing response");
        unsafe {
            let this = self.get_unchecked_mut();
            sys_mutex_lock(&raw mut lock_tcpip_core);
            let err = tcp_write(
                this.pcb,
                bytes.as_ptr(),
                bytes.len() as u16,
                TCP_WRITE_FLAG_COPY,
            );
            if err != LwipError::Ok {
                log_error!("TCP Write Error");
                sys_mutex_unlock(&raw mut lock_tcpip_core);
                return Err(err);
            }
            let err = tcp_output(this.pcb);
            log_info!("tcp_output: {err:?}");
            if err != LwipError::Ok {
                log_error!("tcp_output: {err:?}");
            }
            sys_mutex_unlock(&raw mut lock_tcpip_core);
            Ok(())
        }
    }

    unsafe extern "C" fn _recv(
        arg: *mut c_void,
        _pcb: *mut lwip_pcb,
        pbuf: *mut lwip_pbuf,
        _err: LwipError,
    ) -> LwipError {
        log_info!("_recv()");
        if arg.is_null() {
            return LwipError::Val;
        }
        let conn = unsafe { &*(arg as *const TcpConn<N>) };
        let Ok(pb) = PacketBuffer::try_from(pbuf) else {
            let _ = conn.channel.try_send(None);
            return LwipError::Val;
        };

        let len = pb.total_len();
        match conn.channel.try_send(Some(pb)) {
            Ok(_) => {
                unsafe { tcp_recved(conn.pcb, len) };
                LwipError::Ok
            }
            Err(_) => {
                log_error!("Channel is Full");
                LwipError::Mem
            }
        }
    }

    unsafe extern "C" fn _err(arg: *mut c_void, _err: LwipError) {
        log_info!("_err()");
        if !arg.is_null() {
            log_error!("on_tcp_err");
            let conn = unsafe { &*(arg as *const TcpConn<N>) };
            let _ = conn.channel.try_send(None);
        }
    }

    unsafe extern "C" fn _sent(_arg: *mut c_void, _pcb: *mut lwip_pcb, _len: u16) -> LwipError {
        log_info!("_sent()");
        LwipError::Ok
    }
}

impl<const N: usize> Drop for TcpConn<N> {
    fn drop(&mut self) {
        log_info!("Dropping TcpConn");
        unsafe {
            sys_mutex_lock(&raw mut lock_tcpip_core);
            tcp_recv(self.pcb, None);
            tcp_err(self.pcb, None);
            tcp_sent(self.pcb, None);
            tcp_arg(self.pcb, core::ptr::null_mut());
            let err = tcp_output(self.pcb);
            log_info!("tcp_output: {err:?}");
            if err != LwipError::Ok {
                log_error!("tcp_output: {err:?}");
            }
            let err = tcp_close(self.pcb);
            if err != LwipError::Ok {
                log_error!("tcp_close: {err:?}");
            }
            sys_mutex_unlock(&raw mut lock_tcpip_core);
            self.channel.clear();
        }
    }
}
