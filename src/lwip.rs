use core::{
    ffi::{c_int, c_long, c_void},
    fmt,
    net::{Ipv4Addr, SocketAddrV4},
};

use embassy_time::Timer;

const AF_INET: c_int = 2;
const SOCK_DGRAM: c_int = 2;
const MSG_DONTWAIT: c_int = 0x08;
const SHUT_WR: c_int = 1;
const SOCK_STREAM: c_int = 1;
const SOL_SOCKET: c_int = 0xfff;
const SO_REUSEADDR: c_int = 0x0004;

mod ffi {
    #![allow(unused)]
    use super::sockaddr_in;
    use core::ffi::{c_int, c_long, c_void};

    // `struct sockaddr *` parameters are declared with our concrete type;
    // it's the same pointer at the ABI level. socklen_t is u32 (checked in
    // lwip/sockets.h); ssize_t is i32 on thumbv7em, which matches isize there.
    //
    // SAFETY: every declaration must match the lwIP prototype of the same name
    // (argument types, widths and pointer directions); the symbols are provided
    // by the firmware's lwIP build at link time. All of these return -1 and set
    // errno on failure. Callers are responsible for passing valid descriptors
    // and pointers (see the SAFETY comment at each call site).
    unsafe extern "C" {
        /// Creates a socket; returns its descriptor or -1.
        pub fn lwip_socket(domain: c_int, ty: c_int, protocol: c_int) -> c_int;
        /// Binds socket `s` to the address in `name`.
        pub fn lwip_bind(s: c_int, name: *const sockaddr_in, namelen: u32) -> c_int;
        /// Connects socket `s` to the address in `name`.
        pub fn lwip_connect(s: c_int, name: *const sockaddr_in, namelen: u32) -> c_int;
        /// Writes the socket's bound address to `name`; `namelen` is in/out.
        pub fn lwip_getsockname(s: c_int, name: *mut sockaddr_in, namelen: *mut u32) -> c_int;
        /// Sends `size` bytes from `data` on a connected socket; returns the
        /// byte count or -1.
        pub fn lwip_send(s: c_int, data: *const u8, size: usize, flags: c_int) -> isize;
        /// Receives up to `len` bytes into `mem`; returns the byte count or -1.
        pub fn lwip_recv(s: c_int, mem: *mut u8, len: usize, flags: c_int) -> isize;
        /// Sends `size` bytes from `data` to the address `to`; returns the
        /// byte count or -1.
        pub fn lwip_sendto(
            s: c_int,
            data: *const u8,
            size: usize,
            flags: c_int,
            to: *const sockaddr_in,
            tolen: u32,
        ) -> isize;
        /// Receives up to `len` bytes into `mem` and writes the sender's
        /// address to `from` (`fromlen` is in/out); returns the byte count or -1.
        pub fn lwip_recvfrom(
            s: c_int,
            mem: *mut u8,
            len: usize,
            flags: c_int,
            from: *mut sockaddr_in,
            fromlen: *mut u32,
        ) -> isize;
        /// Closes socket `s`.
        pub fn lwip_close(s: c_int) -> c_int;
        /// Shuts down the read and/or write side of socket `s` (`how`).
        pub fn lwip_shutdown(s: c_int, how: c_int) -> c_int;
        /// Socket ioctl; `argp` must point to the argument type `cmd` expects.
        pub fn lwip_ioctl(s: c_int, cmd: c_long, argp: *mut c_void) -> c_int;
        /// Sets a socket option; `optval` must point to `optlen` readable bytes.
        pub fn lwip_setsockopt(
            s: c_int,
            level: c_int,
            optname: c_int,
            optval: *const c_void,
            optlen: u32,
        ) -> c_int;
        /// Marks socket `s` as passive, accepting up to `backlog` pending
        /// connections.
        pub fn lwip_listen(s: c_int, backlog: c_int) -> c_int;
        /// Accepts a connection, returning a new socket descriptor (or -1) and
        /// writing the peer's address to `addr` (`addrlen` is in/out).
        pub fn lwip_accept(s: c_int, addr: *mut sockaddr_in, addrlen: *mut u32) -> c_int;

        /// newlib: `errno` is `(*__errno())`, per FreeRTOS task when newlib
        /// reentrancy is enabled.
        pub fn __errno() -> *mut c_int;
    }
}

/// Mirror of lwIP's `struct sockaddr_in` (BSD-style, with `sin_len`).
/// Port and address are in network byte order.
#[repr(C)]
#[derive(Clone, Copy)]
struct sockaddr_in {
    sin_len: u8,
    sin_family: u8,
    sin_port: u16,
    sin_addr: u32,
    sin_zero: [u8; 8],
}

const SOCKADDR_IN_LEN: u32 = core::mem::size_of::<sockaddr_in>() as u32;
const _: () = assert!(SOCKADDR_IN_LEN == 16);

impl sockaddr_in {
    /// An all-zero address, used as the out-parameter for lwIP calls that
    /// fill it in.
    const fn zeroed() -> Self {
        Self {
            sin_len: 0,
            sin_family: 0,
            sin_port: 0,
            sin_addr: 0,
            sin_zero: [0; 8],
        }
    }
}

impl From<SocketAddrV4> for sockaddr_in {
    /// Converts to lwIP's layout: port to network byte order, address bytes
    /// kept in network order.
    fn from(a: SocketAddrV4) -> Self {
        Self {
            sin_len: SOCKADDR_IN_LEN as u8,
            sin_family: AF_INET as u8,
            sin_port: a.port().to_be(),
            // Octets already in network order; keep the byte layout as-is.
            sin_addr: u32::from_ne_bytes(a.ip().octets()),
            sin_zero: [0; 8],
        }
    }
}

impl From<sockaddr_in> for SocketAddrV4 {
    /// Converts from lwIP's layout back to a host-order port and `Ipv4Addr`.
    fn from(s: sockaddr_in) -> Self {
        SocketAddrV4::new(
            Ipv4Addr::from(s.sin_addr.to_ne_bytes()),
            u16::from_be(s.sin_port),
        )
    }
}

/// Turns an lwIP return value into a `Result`.
trait ErrorCheck
where
    Self: Sized,
{
    /// `Ok(self)` if the value is non-negative, otherwise `Err` with the
    /// current `errno` (so call it immediately after the lwIP call).
    fn check_for_err(self) -> Result<Self, Error>;
}

impl ErrorCheck for c_int {
    /// See [`ErrorCheck::check_for_err`]; for `int`-returning calls.
    fn check_for_err(self) -> Result<Self, Error> {
        if self < 0 {
            Err(Error::last())
        } else {
            Ok(self)
        }
    }
}

impl ErrorCheck for isize {
    /// See [`ErrorCheck::check_for_err`]; for `ssize_t`-returning calls.
    fn check_for_err(self) -> Result<Self, Error> {
        if self < 0 {
            Err(Error::last())
        } else {
            Ok(self)
        }
    }
}

/// An lwIP socket error, holding the raw `errno` value.
#[derive(Debug)]
#[repr(transparent)]
pub struct Error(c_int);

impl fmt::Display for Error {
    /// Formats the error using its `Debug` representation (the raw errno).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl core::error::Error for Error {}

impl Error {
    /// Classify `errno` after a lwip_* call returned -1. Call immediately
    /// after the failure, on the same task, before anything else (including
    /// logging) can overwrite errno.
    fn last() -> Self {
        // SAFETY: `__errno()` takes no arguments and returns a non-null
        // pointer to the calling task's errno slot (newlib reentrancy), valid
        // for reads while this task runs.
        let err = unsafe { *ffi::__errno() };
        Self(err)
    }

    /// True for errno 11 (`EAGAIN`/`EWOULDBLOCK`): the non-blocking call had
    /// nothing to do yet and should be retried.
    fn would_block(&self) -> bool {
        self.0 == 11
    }
}

/// A non-blocking lwIP UDP socket. Owns the descriptor and closes it on drop.
#[derive(Debug)]
pub struct UdpSocket(c_int);

impl crate::service::UdpSocket for UdpSocket {
    type Error = Error;

    /// Creates an unbound IPv4 UDP socket.
    fn new() -> Result<Self, Self::Error> {
        // SAFETY: `lwip_socket` takes only plain integers; a failure (-1) is
        // turned into an `Err` by `check_for_err`, so `sid` is a valid
        // descriptor that we now own.
        let sid = unsafe { ffi::lwip_socket(AF_INET, SOCK_DGRAM, 0).check_for_err()? };
        Ok(Self(sid))
    }

    /// Binds to `local` and returns the address actually bound (which
    /// differs from `local` when port 0 was requested).
    async fn bind(self, local: SocketAddrV4) -> Result<(SocketAddrV4, Self), Self::Error> {
        let local = sockaddr_in::from(local);
        // SAFETY: `self.0` is a live socket descriptor owned by `self`;
        // `&local` points to a valid `sockaddr_in` of `SOCKADDR_IN_LEN` bytes
        // that outlives the call.
        let _ = unsafe { ffi::lwip_bind(self.0, &local, SOCKADDR_IN_LEN).check_for_err()? };
        // Port 0 means "pick an ephemeral port"; ask lwIP what it chose.
        let mut actual = sockaddr_in::zeroed();
        let mut len = SOCKADDR_IN_LEN;
        // SAFETY: `self.0` is live; `&mut actual` and `&mut len` are valid,
        // writable locals, and `len` holds the size of `actual` as lwIP
        // requires for the in/out length.
        let _ = unsafe { ffi::lwip_getsockname(self.0, &mut actual, &mut len).check_for_err()? };
        Ok((actual.into(), self))
    }

    /// Waits for a datagram (polling every 10 ms while the socket would
    /// block) and returns the sender and the filled part of `buf`.
    async fn receive<'a>(
        &self,
        buf: &'a mut [u8],
    ) -> Result<(SocketAddrV4, &'a [u8]), Self::Error> {
        loop {
            let mut from = sockaddr_in::zeroed();
            let mut from_len = SOCKADDR_IN_LEN;
            // SAFETY: `self.0` is live; `buf` is a valid, writable slice and
            // lwIP writes at most `buf.len()` bytes into it; `&mut from` and
            // `&mut from_len` are valid, writable locals with `from_len` set to
            // the size of `from`. `MSG_DONTWAIT` makes the call non-blocking.
            let written = unsafe {
                ffi::lwip_recvfrom(
                    self.0,
                    buf.as_mut_ptr(),
                    buf.len(),
                    MSG_DONTWAIT,
                    &mut from,
                    &mut from_len,
                )
                .check_for_err()
            };
            match written {
                Ok(written) => {
                    let buf = &buf[..written as usize];
                    return Ok((from.into(), buf));
                }
                Err(err) => {
                    if err.would_block() {
                        Timer::after_millis(10).await;
                        continue;
                    } else {
                        return Err(err);
                    }
                }
            }
        }
    }

    /// Sends `buf` to `remote` as one datagram, retrying (every 10 ms, up to
    /// 10 times) while the socket would block. Returns the bytes sent.
    async fn send(&self, remote: SocketAddrV4, buf: &[u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        const MAX_RETRIES: u32 = 10;
        let mut n: u32 = 0;
        let remote = sockaddr_in::from(remote);
        loop {
            // SAFETY: `self.0` is live; `buf` is a valid, readable slice of
            // `buf.len()` bytes; `&remote` points to a valid `sockaddr_in` of
            // `SOCKADDR_IN_LEN` bytes. Both outlive the call.
            let written = unsafe {
                ffi::lwip_sendto(
                    self.0,
                    buf.as_ptr(),
                    buf.len(),
                    MSG_DONTWAIT,
                    &remote,
                    SOCKADDR_IN_LEN,
                )
                .check_for_err()
            };
            match written {
                Ok(written) => return Ok(written as usize),
                Err(err) => {
                    if err.would_block() {
                        n += 1;
                        if n > MAX_RETRIES {
                            return Err(err);
                        }
                        Timer::after_millis(10).await;
                        continue;
                    } else {
                        return Err(err);
                    }
                }
            }
        }
    }
}

impl Drop for UdpSocket {
    /// Closes the socket descriptor (the result is ignored).
    fn drop(&mut self) {
        // SAFETY: `self.0` is the descriptor this socket owns; nothing else
        // closes it, and `drop` runs at most once.
        unsafe { ffi::lwip_close(self.0) };
    }
}

/// Puts socket `sid` into non-blocking mode (`FIONBIO` = 1).
fn non_blocking_mode(sid: c_int) -> Result<(), Error> {
    const FIONBIO: c_long = 0x8004_667e_u32 as c_long;
    let on = &mut 1u32 as *mut u32 as *mut c_void;
    // SAFETY: `FIONBIO` expects a pointer to a `u32` flag; `on` points to a
    // `u32` temporary that is lifetime-extended to the end of this function,
    // so it is valid for the call. `sid` is a socket descriptor owned by the
    // caller; an invalid one makes lwIP return an error rather than UB.
    let _ = unsafe { ffi::lwip_ioctl(sid, FIONBIO, on).check_for_err()? };
    Ok(())
}

/// A non-blocking lwIP TCP listening socket. Owns the descriptor.
pub struct TcpListener(c_int);

impl crate::service::TcpListener for TcpListener {
    type TcpStream = TcpStream;
    type Error = Error;

    /// Creates an IPv4 TCP socket with `SO_REUSEADDR` set and non-blocking
    /// mode enabled.
    fn new() -> Result<Self, Self::Error> {
        // SAFETY: `lwip_socket` takes only plain integers. Its result is not
        // checked here: a failure (-1) is later rejected by lwIP as an invalid
        // descriptor in `lwip_setsockopt`, which is checked below.
        let sid = unsafe { ffi::lwip_socket(AF_INET, SOCK_STREAM, 0) };
        // SAFETY: `sid` is the descriptor created above. The option value is
        // a pointer to a promoted `'static` `c_int` constant (1), and
        // `optlen` is its exact size.
        let _ = unsafe {
            ffi::lwip_setsockopt(
                sid,
                SOL_SOCKET,
                SO_REUSEADDR,
                (&1 as *const c_int).cast(),
                core::mem::size_of::<c_int>() as u32,
            )
            .check_for_err()?
        };
        non_blocking_mode(sid)?;
        Ok(Self(sid))
    }

    /// Binds to `local`, starts listening (backlog of 1) and returns the
    /// address actually bound (which differs when port 0 was requested).
    async fn bind(self, local: SocketAddrV4) -> Result<(SocketAddrV4, Self), Self::Error> {
        let local = sockaddr_in::from(local);
        // SAFETY: `self.0` is a live socket descriptor owned by `self`;
        // `&local` points to a valid `sockaddr_in` of `SOCKADDR_IN_LEN` bytes
        // that outlives the call.
        let _ = unsafe { ffi::lwip_bind(self.0, &local, SOCKADDR_IN_LEN).check_for_err()? };
        // SAFETY: `self.0` is a live socket descriptor; the other argument is
        // a plain integer.
        let _ = unsafe { ffi::lwip_listen(self.0, 1).check_for_err()? };
        let mut actual = sockaddr_in::zeroed();
        let mut len = SOCKADDR_IN_LEN;
        // SAFETY: `self.0` is live; `&mut actual` and `&mut len` are valid,
        // writable locals, and `len` holds the size of `actual` as lwIP
        // requires for the in/out length.
        let _ = unsafe { ffi::lwip_getsockname(self.0, &mut actual, &mut len).check_for_err()? };
        Ok((actual.into(), self))
    }

    /// Waits for an incoming connection (polling every 10 ms while none is
    /// pending) and returns it as a `TcpStream` that owns the new descriptor.
    async fn accept(&self) -> Result<Self::TcpStream, Self::Error> {
        loop {
            let mut from = sockaddr_in::zeroed();
            let mut len = SOCKADDR_IN_LEN;
            // SAFETY: `self.0` is a live listening socket; `&mut from` and
            // `&mut len` are valid, writable locals with `len` set to the size
            // of `from`. On success the returned descriptor is new and owned by
            // the `TcpStream` we wrap it in.
            let res = unsafe { ffi::lwip_accept(self.0, &mut from, &mut len).check_for_err() };
            match res {
                Ok(sid) => return Ok(TcpStream(sid)),
                Err(err) => {
                    if !err.would_block() {
                        return Err(err);
                    }
                    // Try again as it may have been busy.
                    Timer::after_millis(10).await;
                }
            }
        }
    }
}

/// A non-blocking lwIP TCP connection. Owns the descriptor and closes it
/// on drop.
pub struct TcpStream(c_int);

impl crate::service::TcpStream for TcpStream {
    type Error = Error;

    /// Connects to `remote`, retrying every 10 ms while the call would block.
    async fn connect(remote: SocketAddrV4) -> Result<Self, Self::Error> {
        // SAFETY: `lwip_socket` takes only plain integers. Its result is not
        // checked here: a failure (-1) is later rejected by lwIP as an invalid
        // descriptor in `non_blocking_mode`, which is checked below.
        let sid = unsafe { ffi::lwip_socket(AF_INET, SOCK_STREAM, 0) };
        non_blocking_mode(sid)?;

        let remote = sockaddr_in::from(remote);
        loop {
            // SAFETY: `sid` is the descriptor created above; `&remote` points
            // to a valid `sockaddr_in` of `SOCKADDR_IN_LEN` bytes that outlives
            // the call.
            let err = unsafe { ffi::lwip_connect(sid, &remote, SOCKADDR_IN_LEN) };
            if err == 0 {
                break;
            }
            let err = Error::last();
            if !err.would_block() {
                return Err(err);
            }
            Timer::after_millis(10).await;
        }

        Ok(Self(sid))
    }

    /// Reads available bytes into `buf` (polling every 10 ms while the socket
    /// would block) and returns the filled part. An empty result means the
    /// peer closed the connection.
    async fn read<'a>(&self, buf: &'a mut [u8]) -> Result<&'a [u8], Self::Error> {
        if buf.is_empty() {
            return Ok(buf);
        }
        loop {
            // SAFETY: `self.0` is a live connected socket; `buf` is a valid,
            // writable slice and lwIP writes at most `buf.len()` bytes into it.
            let res = unsafe {
                ffi::lwip_recv(self.0, buf.as_mut_ptr(), buf.len(), MSG_DONTWAIT).check_for_err()
            };
            match res {
                Ok(n) => return Ok(&buf[..n as usize]),
                Err(err) => {
                    if !err.would_block() {
                        return Err(err);
                    }
                    Timer::after_millis(10).await;
                }
            }
        }
    }

    /// Writes some of `buf` (polling every 10 ms while the socket would
    /// block) and returns the number of bytes accepted, which may be less
    /// than `buf.len()`.
    async fn write(&self, buf: &[u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            // SAFETY: `self.0` is a live connected socket; `buf` is a valid,
            // readable slice of `buf.len()` bytes that outlives the call.
            let res = unsafe {
                ffi::lwip_send(self.0, buf.as_ptr(), buf.len(), MSG_DONTWAIT).check_for_err()
            };
            match res {
                Ok(n) => return Ok(n as usize),
                Err(err) => {
                    if !err.would_block() {
                        return Err(err);
                    }
                    Timer::after_millis(10).await;
                }
            }
        }
    }

    /// Shuts down the write side of the connection (signalling end of
    /// data to the peer) while leaving it open for reading.
    async fn finish(&self) -> Result<(), Self::Error> {
        // SAFETY: `self.0` is a live connected socket; the other argument is
        // a plain integer (`SHUT_WR`).
        let _ = unsafe { ffi::lwip_shutdown(self.0, SHUT_WR).check_for_err()? };
        Ok(())
    }
}

impl Drop for TcpStream {
    /// Closes the socket descriptor (the result is ignored).
    fn drop(&mut self) {
        // SAFETY: `self.0` is the descriptor this stream owns; nothing else
        // closes it, and `drop` runs at most once.
        unsafe { ffi::lwip_close(self.0) };
    }
}

// SAFETY: the declaration must match `derusting_local_ipv4` in the glue layer
// (`libderusting.cpp`): no arguments, returning the IPv4 address as a `u32`.
unsafe extern "C" {
    /// Returns the printer's local IPv4 address (octets in network order,
    /// as laid out in memory), or 0 if it has none.
    fn derusting_local_ipv4() -> u32;
}

/// The printer's local IPv4 address from lwIP, or `None` if it has none yet.
pub fn local_ipv4() -> Option<Ipv4Addr> {
    // SAFETY: `derusting_local_ipv4` takes no arguments and just returns a
    // plain integer.
    match unsafe { derusting_local_ipv4() } {
        0 => None,
        a => Some(Ipv4Addr::from(a.to_ne_bytes())),
    }
}
