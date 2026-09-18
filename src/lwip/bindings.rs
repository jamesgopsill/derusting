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
pub struct lwip_netif {
    pub next: *mut lwip_netif,
    pub ip_addr: lwip_ipaddr,
    pub netmask: lwip_ipaddr,
    pub gateway: lwip_ipaddr,
    // ...
}

#[repr(C)]
pub struct sys_mutex_t {
    _opaque: [u8; 0],
}

#[repr(i32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LwipError {
    #[error("Ok: No error, all is ok")]
    Ok = 0,
    #[error("Out of memory error")]
    Mem = -1,
    #[error("Buffer error")]
    Buf = -2,
    #[error("Timeout")]
    Timeout = -3,
    #[error("Routing problem")]
    Rte = -4,
    #[error("Operation in progress")]
    InProgress = -5,
    #[error("Illegal value")]
    Val = -6,
    #[error("Operation would block")]
    WouldBlock = -7,
    #[error("Address in use")]
    Use = -8,
    #[error("Already connecting / already connected")]
    Already = -9,
    #[error("Connection already established")]
    IsConn = -10,
    #[error("Not connected")]
    Conn = -11,
    #[error("Low-level netif error")]
    If = -12,
    #[error("Connection aborted")]
    Abrt = -13,
    #[error("Connection reset")]
    Rst = -14,
    #[error("Connection closed")]
    Clsd = -15,
    #[error("Illegal argument")]
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

pub type TcpSentFn =
    unsafe extern "C" fn(arg: *mut c_void, pcb: *mut lwip_pcb, len: u16) -> LwipError;

pub type TcpConnectedFn =
    unsafe extern "C" fn(arg: *mut c_void, pcb: *mut lwip_pcb, err: LwipError) -> LwipError;

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
    // --- TCP Control ---

    /// Creates a new TCP Protocol Control Block (PCB). Returns NULL if out of memory.
    pub(super) fn tcp_new() -> *mut lwip_pcb;

    /// Closes the TCP connection. Frees the PCB.
    pub(super) fn tcp_close(pcb: *mut lwip_pcb) -> LwipError;

    /// Binds the PCB to a local IP address and port.
    pub(super) fn tcp_bind(pcb: *mut lwip_pcb, ipaddr: *const lwip_ipaddr, port: u16) -> LwipError;

    /// Sets the PCB to LISTEN state. Returns a new PCB pointer (the original is freed).
    /// Backlog limits the number of pending connections.
    pub(super) fn tcp_listen_with_backlog(pcb: *mut lwip_pcb, backlog: u8) -> *mut lwip_pcb;

    /// Sets the custom program argument (void*) that will be passed to all callbacks for this PCB.
    pub(super) fn tcp_arg(pcb: *mut lwip_pcb, arg: *mut c_void);

    pub(super) fn tcp_abort(pcb: *mut lwip_pcb);

    pub(super) fn tcp_connect(
        pcb: *mut lwip_pcb,
        addr: *const lwip_ipaddr,
        port: u16,
        callback: TcpConnectedFn,
    ) -> LwipError;

    // Our own wrapper around their macro
    pub(super) fn derusting_tcp_sndbuf(pcb: *const lwip_pcb) -> u16;

    // --- TCP Callbacks ---

    /// Registers a callback to be called when a new connection is accepted on a listening PCB.
    pub(super) fn tcp_accept(pcb: *mut lwip_pcb, accept: Option<TcpAcceptFn>);

    /// Registers a callback to be called when data arrives on this PCB.
    pub(super) fn tcp_recv(pcb: *mut lwip_pcb, recv: Option<TcpRecvFn>);

    /// Registers a callback for fatal errors. This PCB will be freed by the stack after this call.
    pub(super) fn tcp_err(pcb: *mut lwip_pcb, err: Option<LwipErrFn>);

    /// Registers a callback to be called when the remote host acknowledges sent data.
    pub(super) fn tcp_sent(arg: *mut lwip_pcb, callback: Option<TcpSentFn>);

    // --- TCP Data Handling ---

    /// Enqueues data to be sent. `apiflags` can be TCP_WRITE_FLAG_COPY or TCP_WRITE_FLAG_MORE.
    /// Note: This only queues data; call tcp_output to actually send it.
    pub(super) fn tcp_write(
        pcb: *mut lwip_pcb,
        dataptr: *const u8,
        len: u16,
        apiflags: u8,
    ) -> LwipError;

    /// Forces any enqueued data in the transmit buffer to be sent immediately.
    pub(super) fn tcp_output(pcb: *mut lwip_pcb) -> LwipError;

    /// Must be called by the application when it has processed data.
    /// This increases the TCP receive window. `len` is the number of bytes consumed.
    pub(super) fn tcp_recved(pcb: *mut lwip_pcb, len: u16);

    // --- UDP ---

    /// Creates a new UDP PCB. Returns NULL if out of memory.
    pub(super) fn udp_new() -> *mut lwip_pcb;

    /// Binds a UDP PCB to a local IP address and port.
    pub(super) fn udp_bind(pcb: *mut lwip_pcb, ipaddr: *const lwip_ipaddr, port: u16) -> LwipError;

    /// Registers a callback for incoming UDP packets.
    pub(super) fn udp_recv(pcb: *mut lwip_pcb, recv_fn: Option<UdpRecvFn>, recv_arg: *mut c_void);

    /// Removes and frees the UDP PCB.
    pub(super) fn udp_remove(pcb: *mut lwip_pcb);

    /// Sends a pbuf to a specific IP and port.
    pub(super) fn udp_sendto(
        pcb: *mut lwip_pcb,
        pbuf: *mut lwip_pbuf,
        ip: *const lwip_ipaddr,
        port: u16,
    ) -> LwipError;

    // --- PBUF (Packet Buffer) Management ---

    /// Decrements the reference count of a pbuf. If count hits zero, it is freed.
    /// Returns the number of pbufs actually freed from the chain.
    pub(super) fn pbuf_free(pbuf: *mut lwip_pbuf) -> u8;

    /// Copies data from a pbuf chain starting at `offset` into a destination `ptr`.
    /// Returns the number of bytes actually copied.
    pub(super) fn pbuf_copy_partial(
        pbuf: *const lwip_pbuf,
        ptr: *mut u8,
        len: u16,
        offset: u16,
    ) -> u16;

    /// Allocates a pbuf of the specified type and for the specified layer (header space).
    pub(super) fn pbuf_alloc(layer: PbufLayer, length: u16, pbuf_type: PbufType) -> *mut lwip_pbuf;

    // --- System / Core ---

    /// IP address constant representing "Any" (0.0.0.0).
    pub(super) static ip_addr_any: lwip_ipaddr;

    /// Global mutex used for thread-safety in lwIP's OS mode (CORE_LOCKING).
    pub(super) static mut lock_tcpip_core: sys_mutex_t;

    /// Platform-specific mutex lock implementation.
    pub(super) fn sys_mutex_lock(mutex: *mut sys_mutex_t);

    /// Platform-specific mutex unlock implementation.
    pub(super) fn sys_mutex_unlock(mutex: *mut sys_mutex_t);

    /// Platform-specific sleep function (milliseconds).
    pub(super) fn sys_msleep(ms: u32);

    // --- Netif (Network Interface) ---

    /// The default network interface used for routing when no specific interface matches.
    pub(super) static netif_default: *mut lwip_netif;

    /// LWIP function callback to run in thread.
    pub(super) fn tcpip_callback(
        callback: unsafe extern "C" fn(ctx: *mut c_void),
        ctx: *mut c_void,
    ) -> LwipError;

    pub(super) fn tcp_process_refused_data(pcb: *mut lwip_pcb) -> LwipError;
}
