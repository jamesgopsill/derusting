use core::{ffi::c_void, marker::PhantomPinned, pin::Pin, sync::atomic::Ordering};

use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, signal::Signal,
};
use portable_atomic::AtomicPtr;

use crate::{
    log_error, log_info,
    lwip::{bindings::*, packet_buffer::PacketBuffer},
};

struct ListenContext {
    port: u16,
    listener_ptr: *mut c_void,
    signal: Signal<CriticalSectionRawMutex, Result<*mut lwip_pcb, LwipError>>,
}

impl ListenContext {
    fn as_mut_ptr(&mut self) -> *mut c_void {
        self as *mut _ as *mut c_void
    }
}

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
        // Prevent re-binding if already listening
        if !self.pcb.load(Ordering::Acquire).is_null() {
            return Err(LwipError::Val);
        }
        let ctx = ListenContext {
            port,
            listener_ptr: self.as_mut_ptr(),
            signal: Signal::new(),
        };
        let mut ctx = core::pin::pin!(ctx);
        let err = unsafe { tcpip_callback(Self::_listen, ctx.as_mut_ptr()) };
        if err != LwipError::Ok {
            return Err(err);
        }
        let pcb = ctx.signal.wait().await?;
        self.pcb.store(pcb, Ordering::Release);
        Ok(())
    }

    unsafe extern "C" fn _listen(ctx: *mut c_void) {
        unsafe {
            if ctx.is_null() {
                return;
            }
            let ctx = &mut *(ctx as *mut ListenContext);
            let pcb = tcp_new();
            if pcb.is_null() {
                ctx.signal.signal(Err(LwipError::Mem));
                return;
            }
            let err = tcp_bind(pcb, &ip_addr_any, ctx.port);
            if err != LwipError::Ok {
                tcp_close(pcb);
                ctx.signal.signal(Err(err));
                return;
            }
            let listen_pcb = tcp_listen_with_backlog(pcb, 1);
            if listen_pcb.is_null() {
                // tcp_close(pcb); unsure I think lwip handles this
                ctx.signal.signal(Err(LwipError::Mem));
                return;
            }
            tcp_arg(listen_pcb, ctx.listener_ptr);
            tcp_accept(listen_pcb, Some(Self::_accept));
            ctx.signal.signal(Ok(listen_pcb))
        }
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

        if listener.channel.try_send(pcb).is_err() {
            log_error!("TCP Listener Channel Full");
            unsafe { tcp_abort(pcb) };
            return LwipError::Abrt;
        };

        LwipError::Ok
    }

    pub async fn with_connection<F>(&'static self, fcn: F)
    where
        F: AsyncFnOnce(Pin<&mut TcpConnection<M>>),
    {
        let pcb = self.channel.receive().await;
        let conn = TcpConnection::<M>::new(pcb);
        let mut conn = core::pin::pin!(conn);
        conn.as_mut().attach_callbacks().await;
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

// TODO: We need to send *mut lwip_buf through the channel
// and pin construct the TcpConn on receipt for the channel
// to prevent it moving on the stack.

struct AttachContext {
    pcb: *mut lwip_pcb,
    conn_ptr: *mut c_void,
    signal: Signal<CriticalSectionRawMutex, bool>,
}

impl AttachContext {
    fn as_mut_ptr(&mut self) -> *mut c_void {
        self as *mut _ as *mut c_void
    }
}

struct ResponseContext {
    pcb: *mut lwip_pcb,
    bytes_ptr: *const u8,
    bytes_len: u16,
    signal: Signal<CriticalSectionRawMutex, Result<(), LwipError>>,
}

impl ResponseContext {
    fn as_mut_ptr(&mut self) -> *mut c_void {
        self as *mut _ as *mut c_void
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

    pub async fn attach_callbacks(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };
        let pcb = this.pcb.load(Ordering::Acquire);
        if pcb.is_null() {
            return;
        }
        let mut ctx = AttachContext {
            pcb,
            conn_ptr: this as *mut _ as *mut c_void,
            signal: Signal::new(),
        };
        let err = unsafe { tcpip_callback(Self::_attach_callbacks, ctx.as_mut_ptr()) };
        if err != LwipError::Ok {
            return;
        }
        ctx.signal.wait().await;
    }

    unsafe extern "C" fn _attach_callbacks(ctx: *mut c_void) {
        unsafe {
            if ctx.is_null() {
                return;
            }
            let ctx = &mut *(ctx as *mut AttachContext);
            tcp_arg(ctx.pcb, ctx.conn_ptr);
            tcp_recv(ctx.pcb, Some(Self::_recv));
            tcp_err(ctx.pcb, Some(Self::_err));
            tcp_sent(ctx.pcb, Some(Self::_sent));
            ctx.signal.signal(true);
        }
    }

    pub async fn receive(self: Pin<&Self>) -> Option<PacketBuffer> {
        let pkt = self.channel.receive().await?;
        let len = pkt.total_len();
        let pcb = self.pcb.load(Ordering::Acquire);
        if !pcb.is_null() {
            unsafe {
                sys_mutex_lock(&raw mut lock_tcpip_core);
                let current_pcb = self.pcb.load(Ordering::Relaxed);
                if !current_pcb.is_null() {
                    tcp_recved(current_pcb, len);
                }
                sys_mutex_unlock(&raw mut lock_tcpip_core);
            }
        }
        Some(pkt)
    }

    pub async fn response(self: Pin<&mut Self>, bytes: &[u8]) -> Result<(), LwipError> {
        if bytes.len() > u16::MAX as usize {
            return Err(LwipError::Val);
        }
        let pcb = self.pcb.load(Ordering::Acquire);
        if pcb.is_null() {
            return Err(LwipError::Mem);
        }
        let mut ctx = ResponseContext {
            pcb,
            bytes_ptr: bytes.as_ptr(),
            bytes_len: bytes.len() as u16,
            signal: Signal::new(),
        };
        let err = unsafe { tcpip_callback(Self::_send_response, ctx.as_mut_ptr()) };
        if err != LwipError::Ok {
            return Err(err);
        }
        ctx.signal.wait().await
    }

    unsafe extern "C" fn _send_response(ctx: *mut c_void) {
        unsafe {
            if ctx.is_null() {
                return;
            }
            let ctx = &mut *(ctx as *mut ResponseContext);
            let err = tcp_write(ctx.pcb, ctx.bytes_ptr, ctx.bytes_len, TCP_WRITE_FLAG_COPY);
            if err != LwipError::Ok {
                ctx.signal.signal(Err(err));
                return;
            }
            let err = tcp_output(ctx.pcb);
            if err != LwipError::Ok {
                ctx.signal.signal(Err(err));
                return;
            }
            ctx.signal.signal(Ok(()));
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
            if !pbuf.is_null() {
                unsafe { pbuf_free(pbuf) };
            }
            return LwipError::Val;
        }
        let conn = unsafe { &*(arg as *const TcpConnection<N>) };
        let Ok(pb) = PacketBuffer::try_from(pbuf) else {
            // Should not be the case.
            if !pbuf.is_null() {
                unsafe { pbuf_free(pbuf) };
            }
            let _ = conn.channel.try_send(None);
            return LwipError::Val;
        };

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
