use core::{ffi::c_void, marker::PhantomPinned, pin::Pin};

use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel, signal::Signal};

use crate::{
    log_error, log_info,
    lwip::{bindings::*, packet_buffer::PacketBuffer},
};

struct ListenContext {
    port: u16,
    listener_ptr: *mut c_void,
    signal: Signal<ThreadModeRawMutex, Result<*mut lwip_pcb, LwipError>>,
}

impl ListenContext {
    fn as_mut_ptr(&mut self) -> *mut c_void {
        self as *mut _ as *mut c_void
    }
}

/// Accepts new TCP connections through LWIP.
pub struct TcpListener<const N: usize, const M: usize> {
    pcb: *mut lwip_pcb,
    channel: Channel<ThreadModeRawMutex, *mut lwip_pcb, N>,
    _pin: PhantomPinned,
}

impl<const N: usize, const M: usize> TcpListener<N, M> {
    /// Create a new instance with a sender that passes `*mut lwip_pcb` when
    /// accepting a new connection.
    pub fn new() -> Self {
        Self {
            pcb: core::ptr::null_mut(),
            channel: Channel::new(),
            _pin: PhantomPinned,
        }
    }

    pub async fn listen(self: Pin<&mut Self>, port: u16) -> Result<(), LwipError> {
        let this = unsafe { self.get_unchecked_mut() };
        let mut ctx = ListenContext {
            port,
            listener_ptr: this as *mut _ as *mut c_void,
            signal: Signal::new(),
        };
        unsafe { tcpip_callback(Self::_listen, ctx.as_mut_ptr()) };
        let pcb = ctx.signal.wait().await?;
        this.pcb = pcb;
        Ok(())
    }

    unsafe extern "C" fn _listen(ctx: *mut c_void) {
        unsafe {
            if ctx.is_null() {
                return;
            }
            let ctx = &mut *(ctx as *mut ListenContext);
            let pcb = tcp_new();
            let err = tcp_bind(pcb, &ip_addr_any, ctx.port);
            if err != LwipError::Ok {
                tcp_close(pcb);
                ctx.signal.signal(Err(err));
                return;
            }
            let pcb = tcp_listen_with_backlog(pcb, 1);
            if pcb.is_null() {
                ctx.signal.signal(Err(LwipError::Arg));
                return;
            }
            tcp_arg(pcb, ctx.listener_ptr);
            tcp_accept(pcb, Some(Self::_accept));
            ctx.signal.signal(Ok(pcb))
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
        };

        LwipError::Ok
    }

    pub async fn with_connection<F>(self: Pin<&Self>, fcn: F)
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
            tcp_accept(self.pcb, None);
            tcp_arg(self.pcb, core::ptr::null_mut());
            let err = tcp_close(self.pcb);
            if err != LwipError::Ok {
                log_error!("tcp_close: {err:?}");
            }
            self.channel.clear();
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
    signal: Signal<ThreadModeRawMutex, bool>,
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
    signal: Signal<ThreadModeRawMutex, Result<(), LwipError>>,
}

impl ResponseContext {
    fn as_mut_ptr(&mut self) -> *mut c_void {
        self as *mut _ as *mut c_void
    }
}

pub struct TcpConnection<const N: usize> {
    pcb: *mut lwip_pcb,
    channel: Channel<ThreadModeRawMutex, Option<PacketBuffer>, N>,
    _pin: PhantomPinned,
}

impl<const N: usize> TcpConnection<N> {
    /// Create a new instance of TcpConnection
    pub fn new(pcb: *mut lwip_pcb) -> Self {
        Self {
            pcb,
            channel: Channel::new(),
            _pin: PhantomPinned,
        }
    }

    pub async fn attach_callbacks(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };
        let mut ctx = AttachContext {
            pcb: this.pcb,
            conn_ptr: this as *mut _ as *mut c_void,
            signal: Signal::new(),
        };
        unsafe { tcpip_callback(Self::_attach_callbacks, ctx.as_mut_ptr()) };
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
        self.channel.receive().await
    }

    pub async fn response(self: Pin<&mut Self>, bytes: &[u8]) -> Result<(), LwipError> {
        log_info!("Writing response");
        let this = unsafe { self.get_unchecked_mut() };
        let mut ctx = ResponseContext {
            pcb: this.pcb,
            bytes_ptr: bytes.as_ptr(),
            bytes_len: bytes.len() as u16,
            signal: Signal::new(),
        };
        unsafe { tcpip_callback(Self::_send_response, ctx.as_mut_ptr()) };
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
            return LwipError::Val;
        }
        let conn = unsafe { &*(arg as *const TcpConnection<N>) };
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
            let conn = unsafe { &*(arg as *const TcpConnection<N>) };
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
