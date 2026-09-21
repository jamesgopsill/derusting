use core::{ffi::c_void, marker::PhantomPinned, pin::Pin, sync::atomic::Ordering};

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use portable_atomic::AtomicPtr;

use crate::{
    log_error, log_info,
    lwip::{bindings::*, packet_buffer::PacketBuffer},
};

/// Accepts new TCP connections through LWIP.
pub struct TcpListener<const N: usize, const M: usize> {
    pcb: AtomicPtr<pcb>,
    channel: Channel<CriticalSectionRawMutex, *mut pcb, N>,
}

impl<const N: usize, const M: usize> TcpListener<N, M> {
    /// Create a new instance with a sender that passes `*mut pcb` when
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

    pub async fn listen(&'static self, port: u16) -> Result<(), err_t> {
        // SAFETY: runs inside `async_lwip`, i.e. with `lock_tcpip_core`
        // held, as required by all the `tcp_*` calls below. `self` is
        // `&'static`, so `self.as_mut_ptr()` registered as the listening
        // pcb's `arg` stays valid for as long as the pcb (and therefore
        // this `TcpListener`) exists.
        super::async_lwip(|| unsafe {
            if !self.pcb.load(Ordering::Acquire).is_null() {
                return Err(err_t::Val);
            }
            let pcb = tcp_new();
            if pcb.is_null() {
                return Err(err_t::Mem);
            }
            let err = tcp_bind(pcb, &ip_addr_any, port);
            if err != err_t::Ok {
                tcp_close(pcb);
                return Err(err);
            }
            let listen_pcb = tcp_listen_with_backlog(pcb, 2);
            if listen_pcb.is_null() {
                // tcp_close(pcb); unsure I think lwip handles this
                return Err(err_t::Mem);
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
    ///
    /// # Safety
    /// Called by lwIP as a `tcp_accept_fn`; `arg` must be either null or the
    /// `*mut c_void` registered via `tcp_arg` in `listen()`, i.e. a valid
    /// `*const TcpListener<N, M>`; `pcb`, if non-null, is a freshly accepted
    /// pcb owned by this callback until handed off or aborted below.
    unsafe extern "C" fn _accept(arg: *mut c_void, pcb: *mut pcb, err: err_t) -> err_t {
        log_info!("on_accept");
        if err != err_t::Ok {
            log_error!("TCPError");
            return err_t::Ok;
        }

        if pcb.is_null() {
            log_error!("PCB Null");
            return err_t::Ok;
        }

        if arg.is_null() {
            return err_t::Ok;
        }

        // # Safety
        // We ensure the listener is set as an arg before the callback
        // is called.
        let listener = unsafe { &*(arg as *const TcpListener<N, M>) };

        // Tell lwIP to refuse & buffer incoming packets on this PCB
        // until the application task attaches its real receiver.
        // SAFETY: we're in a lwIP callback (core lock held) and `pcb` is
        // the non-null pcb lwIP just handed us.
        unsafe {
            tcp_arg(pcb, core::ptr::null_mut());
            tcp_recv(pcb, Some(Self::_hold_recv));
        }

        if listener.channel.try_send(pcb).is_err() {
            log_error!("TCP Listener Channel Full");
            // SAFETY: same live `pcb` as above; aborting here is how we
            // relinquish ownership when nobody will accept the connection.
            unsafe { tcp_abort(pcb) };
            return err_t::Ok;
        };

        err_t::Ok
    }

    /// Temporary receiver installed while the PCB is waiting for
    /// the stack-allocated `TcpConnection` to be created and pinned.
    ///
    /// # Safety
    /// Called by lwIP as a `tcp_recv_fn`; all arguments are unused so no
    /// further preconditions apply beyond the standard lwIP calling
    /// context.
    unsafe extern "C" fn _hold_recv(
        _arg: *mut c_void,
        pcb: *mut pcb,
        _pbuf: *mut pbuf,
        _err: err_t,
    ) -> err_t {
        log_info!("[{pcb:p}] _hold_recv()");
        err_t::Mem
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
        // SAFETY: `lock_tcpip_core` is lwIP's own global mutex, valid for
        // the program's lifetime; we lock/unlock it in a single balanced
        // pair around the tcp_* calls below, all of which require the core
        // lock and a non-null pcb (checked via `is_null()` first).
        unsafe {
            sys_mutex_lock(&raw mut lock_tcpip_core);
            let pcb = self.pcb.swap(core::ptr::null_mut(), Ordering::AcqRel);
            if !pcb.is_null() {
                tcp_accept(pcb, None);
                tcp_arg(pcb, core::ptr::null_mut());
                let err = tcp_close(pcb);
                if err != err_t::Ok {
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
    pcb: AtomicPtr<pcb>,
    channel: Channel<CriticalSectionRawMutex, Option<PacketBuffer>, N>,
    _pin: PhantomPinned,
}

impl<const N: usize> TcpConnection<N> {
    /// Create a new instance of TcpConnection
    pub fn new(pcb: *mut pcb) -> Self {
        Self {
            pcb: AtomicPtr::new(pcb),
            channel: Channel::new(),
            _pin: PhantomPinned,
        }
    }

    pub async fn attach_callbacks(self: Pin<&mut Self>) -> Result<(), err_t> {
        log_info!("Attaching Callbacks");
        // SAFETY: we only use `this` to read/register fields and to obtain
        // a stable `*mut c_void` for lwIP callbacks; the pin invariant
        // (this value never moves) is upheld by the caller (`with_connection`
        // keeps it `pin!`'d in a local for the duration it's registered
        // with lwIP).
        let this = unsafe { self.get_unchecked_mut() };
        // SAFETY: runs inside `async_lwip` (core lock held); `pcb` is
        // checked non-null before use, and `this as *mut _ as *mut c_void`
        // is valid for as long as `this` stays pinned (see above).
        super::async_lwip(|| unsafe {
            let pcb = this.pcb.load(Ordering::Acquire);
            if pcb.is_null() {
                return Err(err_t::Val);
            }
            tcp_arg(pcb, this as *mut _ as *mut c_void);
            tcp_recv(pcb, Some(Self::_recv));
            tcp_err(pcb, Some(Self::_err));
            tcp_sent(pcb, Some(Self::_sent));
            if derusting_tcp_has_refused_data(pcb) {
                tcp_process_refused_data(pcb);
            }
            Ok(())
        })
        .await
    }

    pub async fn receive(self: Pin<&Self>) -> Option<PacketBuffer> {
        let pkt = self.channel.receive().await?;
        let len = pkt.total_len();
        let pcb = self.pcb.load(Ordering::Acquire);
        if !pcb.is_null() {
            // SAFETY: runs inside `async_lwip` (core lock held);
            // `current_pcb` is re-checked non-null immediately before use
            // since the pcb can be cleared concurrently by `_err`.
            let _ = super::async_lwip(|| unsafe {
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

    pub async fn response(self: Pin<&mut Self>, bytes: &[u8]) -> Result<(), err_t> {
        if bytes.len() > u16::MAX as usize {
            return Err(err_t::Val);
        }

        // SAFETY: runs inside `async_lwip` (core lock held); `pcb` is
        // checked non-null above, `bytes` is a valid slice for its own
        // length (already bounds-checked against `u16::MAX` above), and
        // `TCP_WRITE_FLAG_COPY` tells lwIP to copy the data rather than
        // retain the pointer.
        super::async_lwip(|| unsafe {
            let pcb = self.pcb.load(Ordering::Acquire);
            if pcb.is_null() {
                return Err(err_t::Mem);
            }
            let err = tcp_write(pcb, bytes.as_ptr(), bytes.len() as u16, TCP_WRITE_FLAG_COPY);
            if err != err_t::Ok {
                return Err(err);
            }
            let err = tcp_output(pcb);
            if err != err_t::Ok {
                return Err(err);
            }
            Ok(())
        })
        .await
    }

    /// # Safety
    /// Called by lwIP as a `tcp_recv_fn`; `arg` must be either null or the
    /// `*mut c_void` registered via `tcp_arg` in `attach_callbacks`, i.e. a
    /// valid `*const TcpConnection<N>`; `pbuf`, if non-null, is a pbuf this
    /// callback takes ownership of (freed directly, or wrapped and handed
    /// to `conn.channel`).
    unsafe extern "C" fn _recv(
        arg: *mut c_void,
        pcb: *mut pcb,
        pbuf: *mut pbuf,
        _err: err_t,
    ) -> err_t {
        log_info!("[{pcb:p}] _recv()");
        if arg.is_null() {
            if !pbuf.is_null() {
                // SAFETY: `pbuf` is non-null and owned by this callback
                // (see fn-level Safety note); freeing it here since there's
                // no connection to hand it to.
                unsafe { pbuf_free(pbuf) };
            }
            return err_t::Val;
        }
        // SAFETY: `arg` is valid per this function's Safety contract.
        let conn = unsafe { &*(arg as *const TcpConnection<N>) };

        if pbuf.is_null() {
            log_info!("Remote closed connection (EOF / FIN received)");
            // Push None to notify readers on `receive()` that stream has ended
            let _ = conn.channel.try_send(None);
            // Per lwIP docs, you MUST return ERR_OK when pbuf is NULL
            return err_t::Ok;
        }

        // Should not fail: `pbuf` was just checked non-null above, which is
        // `PacketBuffer::try_from`'s only failure case.
        let pb = PacketBuffer::try_from(pbuf).unwrap();

        match conn.channel.try_send(Some(pb)) {
            Ok(_) => err_t::Ok,
            Err(_) => {
                log_error!("Channel is Full");
                err_t::Mem
            }
        }
    }

    /// # Safety
    /// Called by lwIP as a `tcp_err_fn`; `arg` must be either null or the
    /// `*mut c_void` registered via `tcp_arg` in `attach_callbacks`. Per
    /// lwIP's contract, the associated pcb has already been freed by the
    /// stack by the time this fires, so we must not touch it — only clear
    /// our cached copy.
    unsafe extern "C" fn _err(arg: *mut c_void, _err: err_t) {
        log_info!("_err()");
        if !arg.is_null() {
            log_error!("on_tcp_err");
            // SAFETY: `arg` is valid per this function's Safety contract.
            let conn = unsafe { &*(arg as *const TcpConnection<N>) };
            conn.pcb.store(core::ptr::null_mut(), Ordering::Release);
            let _ = conn.channel.try_send(None);
        }
    }

    /// # Safety
    /// Called by lwIP as a `tcp_sent_fn`; both arguments are unused so no
    /// further preconditions apply beyond the standard lwIP calling
    /// context.
    unsafe extern "C" fn _sent(_arg: *mut c_void, _pcb: *mut pcb, _len: u16) -> err_t {
        log_info!("_sent()");
        err_t::Ok
    }
}

impl<const N: usize> Drop for TcpConnection<N> {
    fn drop(&mut self) {
        // SAFETY: `blocking_lwip` ensures `lock_tcpip_core` is held for
        // these tcp_* calls; `pcb` is only used after the `is_null()` check
        // below, and is taken via `swap` so no other code path can act on
        // it concurrently.
        let _ = super::blocking_lwip(|| unsafe {
            let pcb = self.pcb.swap(core::ptr::null_mut(), Ordering::AcqRel);
            if !pcb.is_null() {
                // last chance to drain any data.
                if derusting_tcp_has_refused_data(pcb) {
                    tcp_process_refused_data(pcb);
                }
                while let Ok(item) = self.channel.try_receive() {
                    if let Some(pkt) = item {
                        tcp_recved(pcb, pkt.total_len());
                        // pkt drops here -> pbuf_free, as today
                    }
                }
                tcp_recv(pcb, None);
                tcp_err(pcb, None);
                tcp_sent(pcb, None);
                tcp_arg(pcb, core::ptr::null_mut());
                let err = tcp_output(pcb);
                log_info!("tcp_output: {err:?}");
                if err != err_t::Ok {
                    log_error!("tcp_output: {err:?}");
                }
                let err = tcp_close(pcb);
                if err != err_t::Ok {
                    log_error!("tcp_close: {err:?}");
                }
            }
            Ok(())
        });
        self.channel.clear();
    }
}
