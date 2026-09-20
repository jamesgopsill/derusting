use core::{
    ffi::c_void,
    marker::PhantomPinned,
    net::Ipv4Addr,
    pin::{Pin, pin},
    sync::atomic::Ordering,
};

use alloc::format;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, signal::Signal};
use embedded_io::Read;
use heapless::Vec;
use portable_atomic::AtomicPtr;
use uuid::Uuid;

use crate::{
    fs::{self, FResult, File, ReadBytes},
    log_error,
    lwip::{bindings::*, packet_buffer::PacketBuffer},
};

const BUF_CAP: usize = 1024;

pub async fn put_file(guid: Uuid, addr: Ipv4Addr, port: u16) -> Result<(), err_t> {
    let Ok(put) = PutRequest::new(guid, addr, port) else {
        return Err(err_t::Val);
    };
    let put = pin!(put);
    put.send().await
}

pub struct PutRequest {
    guid: Uuid,
    addr: Ipv4Addr,
    port: u16,
    fsize: u32,
    fh: File<ReadBytes>,
    buf: Vec<u8, BUF_CAP>,
    pcb: AtomicPtr<pcb>,
    signal: Signal<ThreadModeRawMutex, err_t>,
    _pin: PhantomPinned,
}

impl PutRequest {
    /// Create a new instance of `PutRequest`
    pub fn new(guid: Uuid, addr: Ipv4Addr, port: u16) -> Result<Self, FResult> {
        let path = fs::make_path(&guid, false);
        let stat = fs::stat(&path)?;
        let Ok(fh) = fs::open(&path, ReadBytes) else {
            return Err(FResult::NoPath);
        };
        Ok(Self {
            guid,
            addr,
            port,
            fsize: stat.fsize,
            fh,
            buf: Vec::new(),
            pcb: AtomicPtr::new(core::ptr::null_mut()),
            signal: Signal::new(),
            _pin: PhantomPinned,
        })
    }

    fn as_mut_ptr(&mut self) -> *mut c_void {
        self as *const _ as *mut c_void
    }

    /// Called from _connected (first write) and _sent (buffer freed up).
    /// Refills from the file up to capacity, then hands as much as the
    /// pcb currently has room for to tcp_write, keeping any remainder queued.
    ///
    /// # Safety
    /// `pcb` must be a live, non-null TCP pcb belonging to this
    /// `PutRequest` (i.e. the one stored in `self.pcb`), and the caller
    /// must hold `lock_tcpip_core` (as lwIP callbacks always do).
    unsafe fn pump(&mut self, pcb: *mut pcb) -> Result<(), err_t> {
        // 1. Top up the staging buffer from the file, bounded by free capacity.
        let free = self.buf.capacity() - self.buf.len();
        if free > 0 {
            let spare = self.buf.spare_capacity_mut(); // &mut [MaybeUninit<u8>]

            // SAFETY: u8 has no invalid bit patterns, so reinterpreting
            // uninitialized MaybeUninit<u8> storage as &mut [u8] is sound
            // *provided* the Read impl only ever writes into the slice and
            // never reads from it before writing — which is the documented
            // contract for Read::read (both std::io and embedded_io). Our
            // fs::File read just fills bytes from the underlying storage, so
            // it upholds that contract.
            let buf = unsafe {
                core::slice::from_raw_parts_mut(spare.as_mut_ptr().cast::<u8>(), spare.len())
            };

            if let Ok(n) = self.fh.read(buf) {
                // SAFETY: we just initialized exactly `n` bytes at the tail
                // via the read above.
                unsafe { self.buf.set_len(self.buf.len() + n) };
            }
        }

        // 2. Only offer tcp_write what it currently has room for.
        // SAFETY: `pcb` is live per this function's Safety contract.
        let sndbuf = unsafe { derusting_tcp_sndbuf(pcb) } as usize;
        let to_send = self.buf.len().min(sndbuf);
        if to_send == 0 {
            return Ok(()); // wait for the next _sent callback
        }

        // SAFETY: caller (`pump`'s own Safety contract, see below) ensures
        // `pcb` is a live TCP pcb; `self.buf.as_ptr()` is valid for
        // `to_send <= self.buf.len()` bytes, and `TCP_WRITE_FLAG_COPY` tells
        // lwIP to copy the data rather than retain the pointer, so it need
        // not outlive this call.
        let err = unsafe { tcp_write(pcb, self.buf.as_ptr(), to_send as u16, TCP_WRITE_FLAG_COPY) };
        if err != err_t::Ok {
            // ERR_MEM here just means "try again next callback" — don't drop data.
            return if err == err_t::Mem { Ok(()) } else { Err(err) };
        }

        // 3. Drop the bytes lwip now owns a copy of, shift the rest down.
        self.buf.copy_within(to_send.., 0);
        self.buf.truncate(self.buf.len() - to_send);

        // SAFETY: `pcb` is the same live pcb used above.
        unsafe { tcp_output(pcb) };
        Ok(())
    }

    pub async fn send(self: Pin<&mut Self>) -> Result<(), err_t> {
        // SAFETY: we never move out of `this` or otherwise violate the
        // pin invariant below; we only use it to obtain field references
        // and a stable `*mut c_void` for the lwIP callbacks, all of which
        // require `Self` to stay put in memory for as long as the pcb is
        // live — guaranteed here because `this` is `Pin<&mut Self>`'s
        // pointee, the same stack slot the caller `pin!`'d.
        let this = unsafe { self.get_unchecked_mut() };
        super::async_lwip(|| unsafe {
            let pcb = this.pcb.load(core::sync::atomic::Ordering::Acquire);
            if pcb.is_null() {
                return Err(err_t::Val);
            }
            // SAFETY (this closure): runs inside `async_lwip`, i.e. on the
            // lwIP thread with `lock_tcpip_core` held, as required by
            // `tcp_new`/`tcp_arg`/`tcp_sent`/`tcp_recv`/`tcp_connect`.
            // `this.as_mut_ptr()` is registered as the pcb's `arg` so the
            // `_connected`/`_sent`/`_recv` callbacks can recover `this`;
            // that's sound only as long as `this` (the pinned `PutRequest`)
            // outlives the pcb, which holds while `send()`'s future is
            // driven to completion (see `Drop` for pcb teardown).
            let pcb = tcp_new();
            if pcb.is_null() {
                return Err(err_t::Mem);
            }
            tcp_arg(pcb, this.as_mut_ptr());
            tcp_sent(pcb, Some(Self::_sent));
            tcp_recv(pcb, Some(Self::_recv));
            let addr = this.addr.to_bits();
            let addr = ip_addr_t { addr };
            let err = tcp_connect(pcb, &addr, this.port, Self::_connected);
            if err == err_t::Ok {
                this.pcb.store(pcb, Ordering::Acquire);
                Ok(())
            } else {
                Err(err)
            }
        })
        .await?;
        let err = this.signal.wait().await;
        if err != err_t::Ok {
            return Err(err);
        }
        Ok(())
    }

    /// # Safety
    /// Called by lwIP as a `tcp_connected_fn`; `arg` must be either null or
    /// the `*mut c_void` previously registered via `tcp_arg` in `send()`,
    /// i.e. a pointer to a still-live, pinned `PutRequest`.
    unsafe extern "C" fn _connected(arg: *mut c_void, pcb: *mut pcb, err: err_t) -> err_t {
        if arg.is_null() {
            return err_t::Mem;
        }
        // SAFETY: `arg` is the pointer registered in `send()`, valid per
        // this function's Safety contract.
        let this = unsafe { &mut *(arg as *mut PutRequest) };
        if err != err_t::Ok {
            this.signal.signal(err);
            return err;
        }
        let headers = format!(
            "PUT / HTTP/1.1\r\n\
guid: {}\r\n\
Content-Type: text/x.gcode\r\n
Content-Length: {}\r\n\
Connection: close\r\n\r\n",
            this.guid, this.fsize
        );
        let _ = this.buf.extend_from_slice(headers.as_bytes());

        // SAFETY: `pcb` is the live pcb lwIP just handed us for this
        // connected event, matching `pump`'s Safety contract; we're on the
        // lwIP thread with the core lock held (lwIP callback context).
        if let Err(err) = unsafe { this.pump(pcb) } {
            this.signal.signal(err);
            return err;
        };

        // SAFETY: `pcb` is still the same live pcb.
        unsafe { tcp_output(pcb) };

        err_t::Ok
    }

    /// # Safety
    /// Called by lwIP as a `tcp_sent_fn`; see `_connected` for the `arg`
    /// contract.
    unsafe extern "C" fn _sent(arg: *mut c_void, pcb: *mut pcb, _len: u16) -> err_t {
        if arg.is_null() {
            return err_t::Mem;
        }
        // SAFETY: see `_connected`.
        let this = unsafe { &mut *(arg as *mut PutRequest) };
        // SAFETY: `pcb` is the live pcb for this sent event; see `pump`'s
        // Safety contract.
        if let Err(err) = unsafe { this.pump(pcb) } {
            this.signal.signal(err);
            return err;
        };
        // SAFETY: `pcb` is still the same live pcb.
        unsafe { tcp_output(pcb) };
        err_t::Ok
    }

    /// # Safety
    /// Called by lwIP as a `tcp_recv_fn`; see `_connected` for the `arg`
    /// contract. `pbuf`, if non-null, is a pbuf this callback takes
    /// ownership of (via `PacketBuffer::try_from`).
    unsafe extern "C" fn _recv(
        arg: *mut c_void,
        _pcb: *mut pcb,
        pbuf: *mut pbuf,
        err: err_t,
    ) -> err_t {
        if arg.is_null() {
            return err_t::Mem;
        }
        // SAFETY: see `_connected`.
        let this = unsafe { &mut *(arg as *mut PutRequest) };
        if err != err_t::Ok {
            this.signal.signal(err);
            return err;
        }
        // Process response.
        let Ok(_pbuf) = PacketBuffer::try_from(pbuf) else {
            // Server closed the connection.
            this.signal.signal(err_t::Clsd);
            return err_t::Ok;
        };
        this.signal.signal(err_t::Ok);
        err_t::Ok
    }
}

impl Drop for PutRequest {
    fn drop(&mut self) {
        // SAFETY: runs under `blocking_lwip`, so `lock_tcpip_core` is held
        // as these tcp_* calls require. `pcb` may be null here (e.g. if
        // `send()` never successfully connected); note lwIP's `tcp_arg`/
        // `tcp_sent`/`tcp_recv`/`tcp_close` are not documented as
        // null-pcb-safe, so this relies on `pcb` being non-null in
        // practice whenever `drop` runs after a successful `send()`.
        let _ = super::blocking_lwip(|| unsafe {
            let pcb = self.pcb.swap(core::ptr::null_mut(), Ordering::AcqRel);
            tcp_arg(pcb, core::ptr::null_mut());
            tcp_sent(pcb, None);
            tcp_recv(pcb, None);
            let err = tcp_close(pcb);
            if err != err_t::Ok {
                log_error!("PutRequest Failed to Close: {err}");
            };
            Ok(())
        });
    }
}
