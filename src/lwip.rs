use core::{
    ffi::{c_int, c_long, c_void},
    fmt,
    net::{Ipv4Addr, SocketAddrV4},
    ops::SubAssign,
};

use embassy_time::{Duration, Ticker, Timer};

use crate::{log_info, lwip::ffi::lwip_socket};

const AF_INET: c_int = 2;
const SOCK_DGRAM: c_int = 2;
const MSG_DONTWAIT: c_int = 0x08;
const SHUT_WR: c_int = 1;
const SOCK_STREAM: c_int = 1;
const SOL_SOCKET: c_int = 0xfff;
const SO_REUSEADDR: c_int = 0x0004;

mod ffi {
    use super::sockaddr_in;
    use core::ffi::{c_int, c_long, c_void};

    // `struct sockaddr *` parameters are declared with our concrete type;
    // it's the same pointer at the ABI level. socklen_t is u32 (checked in
    // lwip/sockets.h); ssize_t is i32 on thumbv7em, which matches isize there.
    unsafe extern "C" {
        pub fn lwip_socket(domain: c_int, ty: c_int, protocol: c_int) -> c_int;
        pub fn lwip_bind(s: c_int, name: *const sockaddr_in, namelen: u32) -> c_int;
        pub fn lwip_connect(s: c_int, name: *const sockaddr_in, namelen: u32) -> c_int;
        pub fn lwip_getsockname(s: c_int, name: *mut sockaddr_in, namelen: *mut u32) -> c_int;
        pub fn lwip_send(s: c_int, data: *const u8, size: usize, flags: c_int) -> isize;
        pub fn lwip_recv(s: c_int, mem: *mut u8, len: usize, flags: c_int) -> isize;
        pub fn lwip_sendto(
            s: c_int,
            data: *const u8,
            size: usize,
            flags: c_int,
            to: *const sockaddr_in,
            tolen: u32,
        ) -> isize;
        pub fn lwip_recvfrom(
            s: c_int,
            mem: *mut u8,
            len: usize,
            flags: c_int,
            from: *mut sockaddr_in,
            fromlen: *mut u32,
        ) -> isize;
        pub fn lwip_close(s: c_int) -> c_int;
        pub fn lwip_shutdown(s: c_int, how: c_int) -> c_int;
        pub fn lwip_ioctl(s: c_int, cmd: c_long, argp: *mut c_void) -> c_int;
        pub fn lwip_setsockopt(
            s: c_int,
            level: c_int,
            optname: c_int,
            optval: *const c_void,
            optlen: u32,
        ) -> c_int;
        pub fn lwip_listen(s: c_int, backlog: c_int) -> c_int;
        pub fn lwip_accept(s: c_int, addr: *mut sockaddr_in, addrlen: *mut u32) -> c_int;

        // newlib: `errno` is `(*__errno())`, per FreeRTOS task when newlib
        // reentrancy is enabled.
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
    fn from(s: sockaddr_in) -> Self {
        SocketAddrV4::new(
            Ipv4Addr::from(s.sin_addr.to_ne_bytes()),
            u16::from_be(s.sin_port),
        )
    }
}

trait ErrorCheck
where
    Self: Sized,
{
    fn check_for_err(self) -> Result<Self, Error>;
}

impl ErrorCheck for c_int {
    fn check_for_err(self) -> Result<Self, Error> {
        if self < 0 {
            Err(Error::last())
        } else {
            Ok(self)
        }
    }
}

impl ErrorCheck for isize {
    fn check_for_err(self) -> Result<Self, Error> {
        if self < 0 {
            Err(Error::last())
        } else {
            Ok(self)
        }
    }
}

#[derive(Debug)]
#[repr(transparent)]
pub struct Error(c_int);

impl fmt::Display for Error {
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
        let err = unsafe { *ffi::__errno() };
        Self(err)
    }

    fn would_block(&self) -> bool {
        self.0 == 11
    }
}

#[derive(Debug)]
pub struct UdpSocket(c_int);

impl crate::service::UdpSocket for UdpSocket {
    type Error = Error;

    fn new() -> Result<Self, Self::Error> {
        let sid = unsafe { ffi::lwip_socket(AF_INET, SOCK_DGRAM, 0).check_for_err()? };
        Ok(Self(sid))
    }

    async fn bind(self, local: SocketAddrV4) -> Result<(SocketAddrV4, Self), Self::Error> {
        let local = sockaddr_in::from(local);
        let _ = unsafe { ffi::lwip_bind(self.0, &local, SOCKADDR_IN_LEN).check_for_err()? };
        // Port 0 means "pick an ephemeral port"; ask lwIP what it chose.
        let mut actual = sockaddr_in::zeroed();
        let mut len = SOCKADDR_IN_LEN;
        let _ = unsafe { ffi::lwip_getsockname(self.0, &mut actual, &mut len).check_for_err()? };
        Ok((actual.into(), self))
    }

    async fn receive<'a>(
        &self,
        buf: &'a mut [u8],
    ) -> Result<(SocketAddrV4, &'a [u8]), Self::Error> {
        loop {
            let mut from = sockaddr_in::zeroed();
            let mut from_len = SOCKADDR_IN_LEN;
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

    async fn send(&self, remote: SocketAddrV4, buf: &[u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        const MAX_RETRIES: u32 = 10;
        let mut n: u32 = 0;
        let remote = sockaddr_in::from(remote);
        loop {
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
    fn drop(&mut self) {
        unsafe { ffi::lwip_close(self.0) };
    }
}

fn non_blocking_mode(sid: c_int) -> Result<(), Error> {
    const FIONBIO: c_long = 0x8004_667e_u32 as c_long;
    let on = &mut 1u32 as *mut u32 as *mut c_void;
    let _ = unsafe { ffi::lwip_ioctl(sid, FIONBIO, on).check_for_err()? };
    Ok(())
}

pub struct TcpListener(c_int);

impl crate::service::TcpListener for TcpListener {
    type TcpStream = TcpStream;
    type Error = Error;

    fn new() -> Result<Self, Self::Error> {
        let sid = unsafe { ffi::lwip_socket(AF_INET, SOCK_STREAM, 0) };
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

    async fn bind(self, local: SocketAddrV4) -> Result<(SocketAddrV4, Self), Self::Error> {
        let local = sockaddr_in::from(local);
        let _ = unsafe { ffi::lwip_bind(self.0, &local, SOCKADDR_IN_LEN).check_for_err()? };
        let _ = unsafe { ffi::lwip_listen(self.0, 1).check_for_err()? };
        let mut actual = sockaddr_in::zeroed();
        let mut len = SOCKADDR_IN_LEN;
        let _ = unsafe { ffi::lwip_getsockname(self.0, &mut actual, &mut len).check_for_err()? };
        Ok((actual.into(), self))
    }

    async fn accept(&self) -> Result<Self::TcpStream, Self::Error> {
        loop {
            let mut from = sockaddr_in::zeroed();
            let mut len = SOCKADDR_IN_LEN;
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

pub struct TcpStream(c_int);

impl crate::service::TcpStream for TcpStream {
    type Error = Error;

    async fn connect(remote: SocketAddrV4) -> Result<Self, Self::Error> {
        let sid = unsafe { ffi::lwip_socket(AF_INET, SOCK_STREAM, 0) };
        non_blocking_mode(sid)?;

        let remote = sockaddr_in::from(remote);
        loop {
            let err = unsafe { ffi::lwip_connect(sid, &remote, SOCKADDR_IN_LEN) };
            if err == 0 {
                break;
            }
            let err = Error::last();
            if err.would_block() {
                return Err(err);
            }
            Timer::after_millis(10).await;
        }

        Ok(Self(sid))
    }

    async fn read<'a>(&self, buf: &'a mut [u8]) -> Result<&'a [u8], Self::Error> {
        if buf.is_empty() {
            return Ok(buf);
        }
        loop {
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

    async fn write(&self, buf: &[u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
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

    async fn finish(&self) -> Result<(), Self::Error> {
        let _ = unsafe { ffi::lwip_shutdown(self.0, SHUT_WR).check_for_err()? };
        Ok(())
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        unsafe { ffi::lwip_close(self.0) };
    }
}

unsafe extern "C" {
    fn derusting_local_ipv4() -> u32;
}

pub fn local_ipv4() -> Option<Ipv4Addr> {
    match unsafe { derusting_local_ipv4() } {
        0 => None,
        a => Some(Ipv4Addr::from(a.to_ne_bytes())),
    }
}
