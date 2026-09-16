use core::{ffi::c_void, marker::PhantomPinned, pin::Pin, sync::atomic::Ordering};

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use portable_atomic::AtomicPtr;

use crate::{
    log_error, log_info,
    lwip::{bindings::*, execute_in_tcpip_thread, packet_buffer::PacketBuffer},
};

/// Accepts new TCP connections through LWIP.
pub struct TcpListener<const N: usize, const M: usize> {
    pcb: AtomicPtr<lwip_pcb>,
    channel: Channel<CriticalSectionRawMutex, *mut lwip_pcb, N>,
}

impl<const N: usize, const M: usize> TcpListener<N, M> {
    /// Create a new instance with a sender that passes `*mut lwip_pcb` when
    /// accepting a new connection.
    pub fn new() -> Self {
        Self {
            pcb: AtomicPtr::new(core::ptr::null_mut()),
            channel: Channel::new(),
        }
    }

    fn as_mut_ptr(&'static self) -> *mut c_void {
        self as *const _ as *mut c_void
    }

    pub async fn listen(&'static self, port: u16) -> Result<(), LwipError> {
        execute_in_tcpip_thread(|| unsafe {
            if !self.pcb.load(Ordering::Acquire).is_null() {
                return Err(LwipError::Val);
            }
            let pcb = tcp_new();
            if pcb.is_null() {
                return Err(LwipError::Mem);
            }
            let err = tcp_bind(pcb, &ip_addr_any, port);
            if err != LwipError::Ok {
                tcp_close(pcb);
                return Err(err);
            }
            let listen_pcb = tcp_listen_with_backlog(pcb, 2);
            if listen_pcb.is_null() {
                // tcp_close(pcb); unsure I think lwip handles this
                return Err(LwipError::Mem);
            }
            tcp_arg(listen_pcb, self.as_mut_ptr());
            tcp_accept(listen_pcb, Some(Self::_accept));
            self.pcb.store(listen_pcb, Ordering::Release);
            Ok(())
        })
        .await
    }

    /// An internal function to handle LWIP callback when accepting
    /// a new function.
    unsafe extern "C" fn _accept(
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

        // # Safety
        // We ensure the listener is set as an arg before the callback
        // is called.
        let listener = unsafe { &*(arg as *const TcpListener<N, M>) };

        // 1. Tell lwIP to refuse & buffer incoming packets on this PCB
        //    until the application task attaches its real receiver.
        unsafe {
            tcp_arg(pcb, core::ptr::null_mut());
            tcp_recv(pcb, Some(Self::_hold_recv));
        }

        if listener.channel.try_send(pcb).is_err() {
            log_error!("TCP Listener Channel Full");
            unsafe { tcp_abort(pcb) };
            return LwipError::Abrt;
        };

        LwipError::Ok
    }

    /// Temporary receiver installed while the PCB is waiting for
    /// the stack-allocated `TcpConnection` to be created and pinned.
    unsafe extern "C" fn _hold_recv(
        _arg: *mut c_void,
        _pcb: *mut lwip_pcb,
        _pbuf: *mut lwip_pbuf,
        _err: LwipError,
    ) -> LwipError {
        log_info!("_hold_recv: refusing data until conn is pinned");
        // Telling lwIP ERR_MEM instructs it to store the pbuf
        // in `pcb->refused_data` instead of discarding it!
        LwipError::Mem
    }

    pub async fn with_connection<F>(&'static self, fcn: F)
    where
        F: AsyncFnOnce(Pin<&mut TcpConnection<M>>),
    {
        let pcb = self.channel.receive().await;
        let conn = TcpConnection::<M>::new(pcb);
        let mut conn = core::pin::pin!(conn);
        let _ = conn.as_mut().attach_callbacks().await;
        fcn(conn).await;
    }
}

impl<const N: usize, const M: usize> Drop for TcpListener<N, M> {
    fn drop(&mut self) {
        unsafe {
            sys_mutex_lock(&raw mut lock_tcpip_core);
            let pcb = self.pcb.swap(core::ptr::null_mut(), Ordering::AcqRel);
            if !pcb.is_null() {
                tcp_accept(pcb, None);
                tcp_arg(pcb, core::ptr::null_mut());
                let err = tcp_close(pcb);
                if err != LwipError::Ok {
                    log_error!("tcp_close: {err:?}");
                    tcp_abort(pcb);
                }
                while let Ok(pending_pcb) = self.channel.try_receive() {
                    if !pending_pcb.is_null() {
                        tcp_abort(pending_pcb);
                    }
                }
            }
            sys_mutex_unlock(&raw mut lock_tcpip_core);
        }
    }
}

pub struct TcpConnection<const N: usize> {
    pcb: AtomicPtr<lwip_pcb>,
    channel: Channel<CriticalSectionRawMutex, Option<PacketBuffer>, N>,
    _pin: PhantomPinned,
}

impl<const N: usize> TcpConnection<N> {
    /// Create a new instance of TcpConnection
    pub fn new(pcb: *mut lwip_pcb) -> Self {
        Self {
            pcb: AtomicPtr::new(pcb),
            channel: Channel::new(),
            _pin: PhantomPinned,
        }
    }

    pub async fn attach_callbacks(self: Pin<&mut Self>) -> Result<(), LwipError> {
        log_info!("Attaching Callbacks");
        let this = unsafe { self.get_unchecked_mut() };
        execute_in_tcpip_thread(|| unsafe {
            let pcb = this.pcb.load(Ordering::Acquire);
            if pcb.is_null() {
                return Err(LwipError::Val);
            }
            tcp_arg(pcb, this as *mut _ as *mut c_void);
            tcp_recv(pcb, Some(Self::_recv));
            tcp_err(pcb, Some(Self::_err));
            tcp_sent(pcb, Some(Self::_sent));
            tcp_process_refused_data(pcb);
            Ok(())
        })
        .await
    }

    pub async fn receive(self: Pin<&Self>) -> Option<PacketBuffer> {
        let pkt = self.channel.receive().await?;
        let len = pkt.total_len();
        let pcb = self.pcb.load(Ordering::Acquire);
        if !pcb.is_null() {
            let _ = execute_in_tcpip_thread(|| unsafe {
                let current_pcb = self.pcb.load(Ordering::Acquire);
                if !current_pcb.is_null() {
                    tcp_recved(current_pcb, len);
                }
                Ok(())
            })
            .await;
        }
        Some(pkt)
    }

    pub async fn response(self: Pin<&mut Self>, bytes: &[u8]) -> Result<(), LwipError> {
        if bytes.len() > u16::MAX as usize {
            return Err(LwipError::Val);
        }

        execute_in_tcpip_thread(|| unsafe {
            let pcb = self.pcb.load(Ordering::Acquire);
            if pcb.is_null() {
                return Err(LwipError::Mem);
            }
            let err = tcp_write(pcb, bytes.as_ptr(), bytes.len() as u16, TCP_WRITE_FLAG_COPY);
            if err != LwipError::Ok {
                return Err(err);
            }
            let err = tcp_output(pcb);
            if err != LwipError::Ok {
                return Err(err);
            }
            Ok(())
        })
        .await
    }

    unsafe extern "C" fn _recv(
        arg: *mut c_void,
        _pcb: *mut lwip_pcb,
        pbuf: *mut lwip_pbuf,
        _err: LwipError,
    ) -> LwipError {
        log_info!("_recv()");
        if arg.is_null() {
            if !pbuf.is_null() {
                unsafe { pbuf_free(pbuf) };
            }
            return LwipError::Val;
        }
        let conn = unsafe { &*(arg as *const TcpConnection<N>) };

        if pbuf.is_null() {
            // TODO: Debug - Should be null when closed but return null
            // early PUT stream so ignore it and it works. Need to work
            // out connection drops.
            log_info!("Remote closed connection (EOF / FIN received)");
            // Push None to notify readers on `receive()` that stream has ended
            // let _ = conn.channel.try_send(None);
            // Per lwIP docs, you MUST return ERR_OK when pbuf is NULL
            return LwipError::Ok;
        }

        // Should not fail.
        let pb = PacketBuffer::try_from(pbuf).unwrap();

        match conn.channel.try_send(Some(pb)) {
            Ok(_) => LwipError::Ok,
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
            let conn = unsafe { &*(arg as *const TcpConnection<N>) };
            conn.pcb.store(core::ptr::null_mut(), Ordering::Release);
            let _ = conn.channel.try_send(None);
        }
    }

    unsafe extern "C" fn _sent(_arg: *mut c_void, _pcb: *mut lwip_pcb, _len: u16) -> LwipError {
        log_info!("_sent()");
        LwipError::Ok
    }
}

impl<const N: usize> Drop for TcpConnection<N> {
    fn drop(&mut self) {
        log_info!("Dropping TcpConn");
        unsafe {
            sys_mutex_lock(&raw mut lock_tcpip_core);
            let pcb = self.pcb.swap(core::ptr::null_mut(), Ordering::AcqRel);
            if !pcb.is_null() {
                tcp_recv(pcb, None);
                tcp_err(pcb, None);
                tcp_sent(pcb, None);
                tcp_arg(pcb, core::ptr::null_mut());
                let err = tcp_output(pcb);
                log_info!("tcp_output: {err:?}");
                if err != LwipError::Ok {
                    log_error!("tcp_output: {err:?}");
                }
                let err = tcp_close(pcb);
                if err != LwipError::Ok {
                    log_error!("tcp_close: {err:?}");
                }
            }
            sys_mutex_unlock(&raw mut lock_tcpip_core);
            self.channel.clear();
        }
    }
}
