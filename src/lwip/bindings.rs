#![allow(unused)]
use core::{ffi::c_void, ptr};

use alloc::vec::Vec;

use crate::{log_error, log_info};

#[repr(C)]
pub struct lwip_pcb {
    _unused: [u8; 0],
}

// NOTE: Check LWIP_IPV6 is turned off so just a ipv4 variant of the struct
#[repr(C)]
pub struct lwip_ipaddr {
    pub addr: u32,
}

#[repr(C)]
pub struct lwip_pbuf {
    pub next: *mut lwip_pbuf,
    pub payload: *mut u8,
    pub tot_len: u16,
    pub len: u16,
    type_internal: u8,
    flags: u8,
    ref_count: u16,
}

#[repr(C)]
pub struct sys_mutex_t {
    _opaque: [u8; 0],
}

#[repr(i32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LwipError {
    Ok = 0,
    Mem = -1,
    Buf = -2,
    Timeout = -3,
    Rte = -4,
    InProgress = -5,
    Val = -6,
    WouldBlock = -7,
    Use = -8,
    Already = -9,
    IsConn = -10,
    Conn = -11,
    If = -12,
    Abrt = -13,
    Rst = -14,
    Clsd = -15,
    Arg = -16,
}

#[allow(clippy::from_over_into)]
impl Into<Result<(), LwipError>> for LwipError {
    fn into(self) -> Result<(), LwipError> {
        if self == LwipError::Ok {
            Ok(())
        } else {
            log_error!("{:?}", self);
            Err(self)
        }
    }
}

pub(super) const TCP_WRITE_FLAG_COPY: u8 = 0x01;
const PBUF_LINK_ENCAPSULATION_HLEN: u32 = 0;
const PBUF_LINK_HLEN: u32 = 14; // Could be an addition eth pad size
const PBUF_IP_HLEN: u32 = 20; // could be 20 or 40  ipv4 vs. v6
const PBUF_TRANSPORT_HLEN: u32 = 20;
const PBUF_TYPE_FLAG_STRUCT_DATA_CONTIGUOUS: u32 = 0x80;
const PBUF_TYPE_FLAG_DATA_VOLATILE: u32 = 0x40;
#[allow(unused)]
const PBUF_TYPE_ALLOC_SRC_MASK: u32 = 0x0F;
const PBUF_ALLOC_FLAG_RX: u32 = 0x0100;
const PBUF_ALLOC_FLAG_DATA_CONTIGUOUS: u32 = 0x0200;
const PBUF_TYPE_ALLOC_SRC_MASK_STD_HEAP: u32 = 0x00;
const PBUF_TYPE_ALLOC_SRC_MASK_STD_MEMP_PBUF: u32 = 0x01;
const PBUF_TYPE_ALLOC_SRC_MASK_STD_MEMP_PBUF_POOL: u32 = 0x02;
#[allow(unused)]
const PBUF_TYPE_ALLOC_SRC_MASK_APP_MIN: u32 = 0x03;

pub type TcpAcceptFn =
    unsafe extern "C" fn(arg: *mut c_void, pcb: *mut lwip_pcb, err: LwipError) -> LwipError;
pub type TcpRecvFn = unsafe extern "C" fn(
    arg: *mut c_void,
    pcb: *mut lwip_pcb,
    pbuf: *mut lwip_pbuf,
    err: LwipError,
) -> LwipError;
pub type UdpRecvFn = unsafe extern "C" fn(
    arg: *mut c_void,
    pcb: *mut lwip_pcb,
    pbuf: *mut lwip_pbuf,
    addr: *const lwip_ipaddr,
    port: u16,
);

pub type LwipErrFn = unsafe extern "C" fn(arg: *mut c_void, err: LwipError);

#[repr(u32)]
#[allow(unused)]
pub enum PbufLayer {
    Transport = PBUF_LINK_ENCAPSULATION_HLEN + PBUF_LINK_HLEN + PBUF_IP_HLEN + PBUF_TRANSPORT_HLEN,
    Ip = PBUF_LINK_ENCAPSULATION_HLEN + PBUF_LINK_HLEN + PBUF_IP_HLEN,
    Link = PBUF_LINK_ENCAPSULATION_HLEN + PBUF_LINK_HLEN,
    Raw = PBUF_LINK_ENCAPSULATION_HLEN,
}

#[repr(u32)]
#[allow(unused)]
pub enum PbufType {
    Ram = PBUF_ALLOC_FLAG_DATA_CONTIGUOUS
        | PBUF_TYPE_FLAG_STRUCT_DATA_CONTIGUOUS
        | PBUF_TYPE_ALLOC_SRC_MASK_STD_HEAP,
    Rom = PBUF_TYPE_ALLOC_SRC_MASK_STD_MEMP_PBUF,
    Ref = PBUF_TYPE_FLAG_DATA_VOLATILE | PBUF_TYPE_ALLOC_SRC_MASK_STD_MEMP_PBUF,
    Pool = PBUF_ALLOC_FLAG_RX
        | PBUF_TYPE_FLAG_STRUCT_DATA_CONTIGUOUS
        | PBUF_TYPE_ALLOC_SRC_MASK_STD_MEMP_PBUF_POOL,
}

unsafe extern "C" {
    // TCP
    pub(super) fn tcp_new() -> *mut lwip_pcb;
    pub(super) fn tcp_close(pcb: *mut lwip_pcb) -> LwipError;
    pub(super) fn tcp_bind(pcb: *mut lwip_pcb, ipaddr: *const lwip_ipaddr, port: u16) -> LwipError;
    pub(super) fn tcp_listen_with_backlog(pcb: *mut lwip_pcb, backlog: u8) -> *mut lwip_pcb;
    pub(super) fn tcp_arg(pcb: *mut lwip_pcb, arg: *mut c_void);
    // Callbacks
    pub(super) fn tcp_accept(pcb: *mut lwip_pcb, accept: Option<TcpAcceptFn>);
    pub(super) fn tcp_recv(pcb: *mut lwip_pcb, recv: Option<TcpRecvFn>);
    pub(super) fn tcp_err(pcb: *mut lwip_pcb, err: Option<LwipErrFn>);
    // Data
    pub(super) fn tcp_write(
        pcb: *mut lwip_pcb,
        dataptr: *const u8,
        len: u16,
        apiflags: u8,
    ) -> LwipError;
    pub(super) fn tcp_output(pcb: *mut lwip_pcb) -> LwipError;
    pub(super) fn tcp_recved(pcb: *mut lwip_pcb, len: u16);
    // UDP
    pub(super) fn udp_new() -> *mut lwip_pcb;
    pub(super) fn udp_bind(pcb: *mut lwip_pcb, ipaddr: *const lwip_ipaddr, port: u16) -> LwipError;
    pub(super) fn udp_recv(pcb: *mut lwip_pcb, recv_fn: Option<UdpRecvFn>, recv_arg: *mut c_void);
    pub(super) fn udp_remove(pcb: *mut lwip_pcb);
    pub(super) fn udp_sendto(
        pcb: *mut lwip_pcb,
        pbuf: *mut lwip_pbuf,
        ip: *const lwip_ipaddr,
        port: u16,
    ) -> LwipError;
    // PBUF
    pub(super) fn pbuf_free(pbuf: *mut lwip_pbuf) -> u8;
    pub(super) fn pbuf_copy_partial(
        pbuf: *const lwip_pbuf,
        ptr: *mut u8,
        len: u16,
        offset: u16,
    ) -> u16;
    pub(super) fn pbuf_alloc(layer: PbufLayer, length: u16, pbuf_type: PbufType) -> *mut lwip_pbuf;
    // statics
    pub(super) static ip_addr_any: lwip_ipaddr;
    pub(super) static mut lock_tcpip_core: sys_mutex_t;
    // core
    pub(super) fn sys_mutex_lock(mutex: *mut sys_mutex_t);
    pub(super) fn sys_mutex_unlock(mutex: *mut sys_mutex_t);
}

/*
pub struct TcpProtocolControlBlock {
    inner: *mut lwip_pcb,
}

impl TryFrom<*mut lwip_pcb> for TcpProtocolControlBlock {
    type Error = ();
    fn try_from(value: *mut lwip_pcb) -> Result<Self, ()> {
        if value.is_null() {
            Err(())
        } else {
            Ok(Self { inner: value })
        }
    }
}

impl TcpProtocolControlBlock {
    pub fn as_mut_ptr(&self) -> *mut lwip_pcb {
        self.inner
    }

    pub fn arg(&self, arg: *mut c_void) {
        unsafe { tcp_arg(self.as_mut_ptr(), arg) };
    }

    pub fn recv(&self, callback: Option<TcpRecvFn>) {
        unsafe { tcp_recv(self.as_mut_ptr(), callback) };
    }

    pub fn err(&self, callback: Option<LwipErrFn>) {
        unsafe { tcp_err(self.as_mut_ptr(), callback) };
    }

    pub fn close(self) -> Result<(), LwipError> {
        let err = unsafe { tcp_close(self.as_mut_ptr()) };
        err.into()
    }

    pub fn recved(&self, len: u16) {
        unsafe { tcp_recved(self.as_mut_ptr(), len) };
    }

    pub fn write(&self, slice: &[u8]) -> Result<(), LwipError> {
        // TODO: check the u16 conversion (suppose it will just clip the data)
        let err = unsafe {
            tcp_write(
                self.as_mut_ptr(),
                slice.as_ptr(),
                slice.len() as u16,
                TCP_WRITE_FLAG_COPY,
            )
        };
        err.into()
    }

    pub fn output(&self) -> Result<(), LwipError> {
        let err = unsafe { tcp_output(self.as_mut_ptr()) };
        err.into()
    }

    pub fn bind(&self, port: u16) -> Result<(), LwipError> {
        let err = unsafe { tcp_bind(self.as_mut_ptr(), &ip_addr_any, port) };
        err.into()
    }

    pub fn accept(&self, callback: Option<TcpAcceptFn>) {
        unsafe { tcp_accept(self.as_mut_ptr(), callback) };
    }

    pub fn listen_with_backlog(&self, backlog: u8) -> Result<TcpProtocolControlBlock, ()> {
        let pcb = unsafe { tcp_listen_with_backlog(self.as_mut_ptr(), backlog) };
        TcpProtocolControlBlock::try_from(pcb)
    }
}

*/
