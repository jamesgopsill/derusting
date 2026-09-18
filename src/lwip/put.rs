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
    lwip::{
        bindings::{
            LwipError, TCP_WRITE_FLAG_COPY, derusting_tcp_sndbuf, lock_tcpip_core, lwip_ipaddr,
            lwip_pbuf, lwip_pcb, sys_mutex_lock, sys_mutex_unlock, tcp_arg, tcp_close, tcp_connect,
            tcp_new, tcp_output, tcp_recv, tcp_sent, tcp_write,
        },
        execute_in_tcpip_thread,
        packet_buffer::PacketBuffer,
    },
};

const BUF_CAP: usize = 1024;

pub async fn put_file(guid: Uuid, addr: Ipv4Addr, port: u16) -> Result<(), LwipError> {
    let Ok(put) = PutRequest::new(guid, addr, port) else {
        return Err(LwipError::Val);
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
    pcb: AtomicPtr<lwip_pcb>,
    signal: Signal<ThreadModeRawMutex, LwipError>,
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
    unsafe fn pump(&mut self, pcb: *mut lwip_pcb) -> Result<(), LwipError> {
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
        let sndbuf = unsafe { derusting_tcp_sndbuf(pcb) } as usize;
        let to_send = self.buf.len().min(sndbuf);
        if to_send == 0 {
            return Ok(()); // wait for the next _sent callback
        }

        let err = unsafe { tcp_write(pcb, self.buf.as_ptr(), to_send as u16, TCP_WRITE_FLAG_COPY) };
        if err != LwipError::Ok {
            // ERR_MEM here just means "try again next callback" — don't drop data.
            return if err == LwipError::Mem {
                Ok(())
            } else {
                Err(err)
            };
        }

        // 3. Drop the bytes lwip now owns a copy of, shift the rest down.
        self.buf.copy_within(to_send.., 0);
        self.buf.truncate(self.buf.len() - to_send);

        unsafe { tcp_output(pcb) };
        Ok(())
    }

    pub async fn send(self: Pin<&mut Self>) -> Result<(), LwipError> {
        let this = unsafe { self.get_unchecked_mut() };
        execute_in_tcpip_thread(|| unsafe {
            let pcb = this.pcb.load(core::sync::atomic::Ordering::Acquire);
            if pcb.is_null() {
                return Err(LwipError::Val);
            }
            let pcb = tcp_new();
            if pcb.is_null() {
                return Err(LwipError::Mem);
            }
            tcp_arg(pcb, this.as_mut_ptr());
            tcp_sent(pcb, Some(Self::_sent));
            tcp_recv(pcb, Some(Self::_recv));
            let addr = this.addr.to_bits();
            let addr = lwip_ipaddr { addr };
            let err = tcp_connect(pcb, &addr, this.port, Self::_connected);
            if err != LwipError::Ok {
                return Err(err);
            }
            Ok(())
        })
        .await?;
        let err = this.signal.wait().await;
        if err != LwipError::Ok {
            return Err(err);
        }
        Ok(())
    }

    unsafe extern "C" fn _connected(
        arg: *mut c_void,
        pcb: *mut lwip_pcb,
        err: LwipError,
    ) -> LwipError {
        if arg.is_null() {
            return LwipError::Mem;
        }
        let this = unsafe { &mut *(arg as *mut PutRequest) };
        if err != LwipError::Ok {
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

        if let Err(err) = unsafe { this.pump(pcb) } {
            this.signal.signal(err);
            return err;
        };

        unsafe { tcp_output(pcb) };

        LwipError::Ok
    }

    unsafe extern "C" fn _sent(arg: *mut c_void, pcb: *mut lwip_pcb, _len: u16) -> LwipError {
        if arg.is_null() {
            return LwipError::Mem;
        }
        let this = unsafe { &mut *(arg as *mut PutRequest) };
        if let Err(err) = unsafe { this.pump(pcb) } {
            this.signal.signal(err);
            return err;
        };
        unsafe { tcp_output(pcb) };
        LwipError::Ok
    }

    unsafe extern "C" fn _recv(
        arg: *mut c_void,
        _pcb: *mut lwip_pcb,
        pbuf: *mut lwip_pbuf,
        err: LwipError,
    ) -> LwipError {
        if arg.is_null() {
            return LwipError::Mem;
        }
        let this = unsafe { &mut *(arg as *mut PutRequest) };
        if err != LwipError::Ok {
            this.signal.signal(err);
            return err;
        }
        // Process response.
        let Ok(_pbuf) = PacketBuffer::try_from(pbuf) else {
            // Server closed the connection.
            this.signal.signal(LwipError::Clsd);
            return LwipError::Ok;
        };
        this.signal.signal(LwipError::Ok);
        LwipError::Ok
    }
}

impl Drop for PutRequest {
    fn drop(&mut self) {
        unsafe {
            sys_mutex_lock(&raw mut lock_tcpip_core);
            let pcb = self.pcb.swap(core::ptr::null_mut(), Ordering::AcqRel);
            tcp_arg(pcb, core::ptr::null_mut());
            tcp_sent(pcb, None);
            tcp_recv(pcb, None);
            let err = tcp_close(pcb);
            if err != LwipError::Ok {
                log_error!("PutRequest Failed to Close: {err}");
            };
            sys_mutex_unlock(&raw mut lock_tcpip_core);
        }
    }
}
