#![allow(unused, non_camel_case_types)]
use core::{ffi::c_void, ptr};

use alloc::vec::Vec;

use crate::{log_error, log_info};

/// Opaque handle to an lwIP protocol control block (TCP or UDP).
#[repr(C)]
pub struct pcb {
    _unused: [u8; 0],
}

// NOTE: Check LWIP_IPV6 is turned off so just a ipv4 variant of the struct
#[repr(C)]
pub struct ip_addr_t {
    pub addr: u32,
}

/// Mirrors lwIP's `pbuf` struct: a node in a chain of packet buffers.
#[repr(C)]
pub struct pbuf {
    pub next: *mut pbuf,
    pub payload: *mut u8,
    pub tot_len: u16,
    pub len: u16,
    type_internal: u8,
    flags: u8,
    ref_count: u16,
}

/// Mirrors (the leading fields of) lwIP's `netif` struct: a network
/// interface.
#[repr(C)]
pub struct netif {
    pub next: *mut netif,
    pub ip_addr: ip_addr_t,
    pub netmask: ip_addr_t,
    pub gateway: ip_addr_t,
    // ...
}

/// Opaque handle to lwIP's platform mutex type.
#[repr(C)]
pub struct sys_mutex_t {
    _opaque: [u8; 0],
}

/// Mirrors lwIP's `err_t` error codes, returned by most lwIP API calls.
#[repr(i8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, thiserror::Error)]
pub enum err_t {
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

const _: () = assert!(core::mem::size_of::<err_t>() == 1);

pub(super) const TCP_WRITE_FLAG_COPY: u8 = 0x01;
const PBUF_LINK_ENCAPSULATION_HLEN: u32 = 0;
const PBUF_LINK_HLEN: u32 = 14; // Could be an addition eth pad size
const PBUF_IP_HLEN: u32 = 20; // could be 20 or 40  ipv4 vs. v6
const PBUF_TRANSPORT_HLEN: u32 = 20;
const PBUF_TYPE_FLAG_STRUCT_DATA_CONTIGUOUS: u32 = 0x80;
const PBUF_TYPE_FLAG_DATA_VOLATILE: u32 = 0x40;
const PBUF_TYPE_ALLOC_SRC_MASK: u32 = 0x0F;
const PBUF_ALLOC_FLAG_RX: u32 = 0x0100;
const PBUF_ALLOC_FLAG_DATA_CONTIGUOUS: u32 = 0x0200;
const PBUF_TYPE_ALLOC_SRC_MASK_STD_HEAP: u32 = 0x00;
const PBUF_TYPE_ALLOC_SRC_MASK_STD_MEMP_PBUF: u32 = 0x01;
const PBUF_TYPE_ALLOC_SRC_MASK_STD_MEMP_PBUF_POOL: u32 = 0x02;
const PBUF_TYPE_ALLOC_SRC_MASK_APP_MIN: u32 = 0x03;

/// Callback signature lwIP invokes via `tcp_accept` when a new connection
/// arrives on a listening pcb.
pub type tcp_accept_fn = unsafe extern "C" fn(arg: *mut c_void, pcb: *mut pcb, err: err_t) -> err_t;

/// Callback signature lwIP invokes via `tcp_recv` when data (or EOF, as a
/// null `pbuf`) arrives on a pcb.
pub type tcp_recv_fn =
    unsafe extern "C" fn(arg: *mut c_void, pcb: *mut pcb, pbuf: *mut pbuf, err: err_t) -> err_t;

/// Callback signature lwIP invokes via `tcp_sent` when previously written
/// data has been acknowledged by the remote host.
pub type tcp_sent_fn = unsafe extern "C" fn(arg: *mut c_void, pcb: *mut pcb, len: u16) -> err_t;

/// Callback signature lwIP invokes via `tcp_connect` once the outbound
/// connection completes (or fails).
pub type tcp_connected_fn =
    unsafe extern "C" fn(arg: *mut c_void, pcb: *mut pcb, err: err_t) -> err_t;

/// Callback signature lwIP invokes via `tcp_err` on a fatal pcb error; the
/// pcb itself has already been freed by lwIP when this fires.
pub type tcp_err_fn = unsafe extern "C" fn(arg: *mut c_void, err: err_t);

/// Callback signature lwIP invokes via `udp_recv` when a UDP datagram
/// arrives.
pub type udp_recv_fn = unsafe extern "C" fn(
    arg: *mut c_void,
    pcb: *mut pcb,
    pbuf: *mut pbuf,
    addr: *const ip_addr_t,
    port: u16,
);

/// Callback signature for a fatal UDP pcb error (unused by lwIP's UDP API
/// today, but part of the binding surface for parity with TCP).
pub type udp_err_fn = unsafe extern "C" fn(arg: *mut c_void, err: err_t);

/// The protocol layer to reserve header space for when allocating a pbuf
/// with `pbuf_alloc`.
#[repr(u32)]
pub enum pbuf_layer {
    Transport = PBUF_LINK_ENCAPSULATION_HLEN + PBUF_LINK_HLEN + PBUF_IP_HLEN + PBUF_TRANSPORT_HLEN,
    Ip = PBUF_LINK_ENCAPSULATION_HLEN + PBUF_LINK_HLEN + PBUF_IP_HLEN,
    Link = PBUF_LINK_ENCAPSULATION_HLEN + PBUF_LINK_HLEN,
    Raw = PBUF_LINK_ENCAPSULATION_HLEN,
}

/// How/where a pbuf's backing storage is allocated, passed to
/// `pbuf_alloc`.
#[repr(u32)]
pub enum pbuf_type {
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
    // -- Own wrappers

    /// Returns how many bytes are currently free in the pcb's send buffer.
    pub(super) fn derusting_tcp_sndbuf(pcb: *const pcb) -> u16;

    /// Whether the calling task already holds lwIP's `lock_tcpip_core`
    /// mutex.
    pub(super) fn derusting_holds_tcpip_core_lock() -> bool;

    /// Whether the pcb is currently holding data lwIP refused to deliver
    /// (i.e. there is buffered data waiting on `tcp_process_refused_data`).
    pub(super) fn derusting_tcp_has_refused_data(pcb: *const pcb) -> bool;

    // --- TCP Control ---

    /// Creates a new TCP Protocol Control Block (PCB). Returns NULL if out of memory.
    pub(super) fn tcp_new() -> *mut pcb;

    /// Closes the TCP connection. Frees the PCB.
    pub(super) fn tcp_close(pcb: *mut pcb) -> err_t;

    /// Binds the PCB to a local IP address and port.
    pub(super) fn tcp_bind(pcb: *mut pcb, ipaddr: *const ip_addr_t, port: u16) -> err_t;

    /// Sets the PCB to LISTEN state. Returns a new PCB pointer (the original is freed).
    /// Backlog limits the number of pending connections.
    pub(super) fn tcp_listen_with_backlog(pcb: *mut pcb, backlog: u8) -> *mut pcb;

    /// Sets the custom program argument (void*) that will be passed to all callbacks for this PCB.
    pub(super) fn tcp_arg(pcb: *mut pcb, arg: *mut c_void);

    /// Aborts the connection and frees the PCB, sending a RST to the peer.
    pub(super) fn tcp_abort(pcb: *mut pcb);

    /// Opens an outbound connection to `addr`:`port`; `callback` fires once
    /// it completes or fails.
    pub(super) fn tcp_connect(
        pcb: *mut pcb,
        addr: *const ip_addr_t,
        port: u16,
        callback: tcp_connected_fn,
    ) -> err_t;

    // --- TCP Callbacks ---

    /// Registers a callback to be called when a new connection is accepted on a listening PCB.
    pub(super) fn tcp_accept(pcb: *mut pcb, accept: Option<tcp_accept_fn>);

    /// Registers a callback to be called when data arrives on this PCB.
    pub(super) fn tcp_recv(pcb: *mut pcb, recv: Option<tcp_recv_fn>);

    /// Registers a callback for fatal errors. This PCB will be freed by the stack after this call.
    pub(super) fn tcp_err(pcb: *mut pcb, err: Option<tcp_err_fn>);

    /// Registers a callback to be called when the remote host acknowledges sent data.
    pub(super) fn tcp_sent(arg: *mut pcb, callback: Option<tcp_sent_fn>);

    // --- TCP Data Handling ---

    /// Enqueues data to be sent. `apiflags` can be TCP_WRITE_FLAG_COPY or TCP_WRITE_FLAG_MORE.
    /// Note: This only queues data; call tcp_output to actually send it.
    pub(super) fn tcp_write(pcb: *mut pcb, dataptr: *const u8, len: u16, apiflags: u8) -> err_t;

    /// Forces any enqueued data in the transmit buffer to be sent immediately.
    pub(super) fn tcp_output(pcb: *mut pcb) -> err_t;

    /// Must be called by the application when it has processed data.
    /// This increases the TCP receive window. `len` is the number of bytes consumed.
    pub(super) fn tcp_recved(pcb: *mut pcb, len: u16);

    // --- UDP ---

    /// Creates a new UDP PCB. Returns NULL if out of memory.
    pub(super) fn udp_new() -> *mut pcb;

    /// Binds a UDP PCB to a local IP address and port.
    pub(super) fn udp_bind(pcb: *mut pcb, ipaddr: *const ip_addr_t, port: u16) -> err_t;

    /// Registers a callback for incoming UDP packets.
    pub(super) fn udp_recv(pcb: *mut pcb, recv_fn: Option<udp_recv_fn>, recv_arg: *mut c_void);

    /// Removes and frees the UDP PCB.
    pub(super) fn udp_remove(pcb: *mut pcb);

    /// Sends a pbuf to a specific IP and port.
    pub(super) fn udp_sendto(
        pcb: *mut pcb,
        pbuf: *mut pbuf,
        ip: *const ip_addr_t,
        port: u16,
    ) -> err_t;

    // --- PBUF (Packet Buffer) Management ---

    /// Decrements the reference count of a pbuf. If count hits zero, it is freed.
    /// Returns the number of pbufs actually freed from the chain.
    pub(super) fn pbuf_free(pbuf: *mut pbuf) -> u8;

    /// Copies data from a pbuf chain starting at `offset` into a destination `ptr`.
    /// Returns the number of bytes actually copied.
    pub(super) fn pbuf_copy_partial(pbuf: *const pbuf, ptr: *mut u8, len: u16, offset: u16) -> u16;

    /// Allocates a pbuf of the specified type and for the specified layer (header space).
    pub(super) fn pbuf_alloc(layer: pbuf_layer, length: u16, pbuf_type: pbuf_type) -> *mut pbuf;

    // --- System / Core ---

    /// IP address constant representing "Any" (0.0.0.0).
    pub(super) static ip_addr_any: ip_addr_t;

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
    pub(super) static netif_default: *mut netif;

    /// LWIP function callback to run in thread.
    pub(super) fn tcpip_callback(
        callback: unsafe extern "C" fn(ctx: *mut c_void),
        ctx: *mut c_void,
    ) -> err_t;

    /// Re-delivers data previously refused by the receive callback (see
    /// `derusting_tcp_has_refused_data`) once the application is ready.
    pub(super) fn tcp_process_refused_data(pcb: *mut pcb) -> err_t;
}
