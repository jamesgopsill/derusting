use core::{
    ffi::c_void,
    net::Ipv4Addr,
    sync::atomic::{AtomicPtr, Ordering},
};

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};

use crate::{
    log_error,
    lwip::{bindings::*, packet_buffer::PacketBuffer},
};

pub struct UdpSocket<const N: usize> {
    pcb: AtomicPtr<pcb>,
    channel: Channel<CriticalSectionRawMutex, (Ipv4Addr, PacketBuffer), N>,
}

// SAFETY: the only lwIP-mutating operations on a `UdpSocket` are
// `udp_new`/`udp_bind`/`udp_recv`/`udp_remove`/`udp_sendto`, all of which go
// through `async_lwip`/`blocking_lwip` and therefore always run with
// `lock_tcpip_core` held, so sharing/moving a `UdpSocket` across tasks
// (which all run on the single Embassy executor thread here) doesn't race
// lwIP's internal state. The `AtomicPtr` field also makes cross-task
// access to `pcb` itself data-race-free.
unsafe impl<const N: usize> Sync for UdpSocket<N> {}
unsafe impl<const N: usize> Send for UdpSocket<N> {}

impl<const N: usize> UdpSocket<N> {
    /// Create a new instance of UDP socket.
    pub fn new() -> Self {
        Self {
            pcb: AtomicPtr::new(core::ptr::null_mut()),
            channel: Channel::new(),
        }
    }

    /// An internal function to create a pointer to `self`
    /// that is used to set up the LWIP callbacks. `self` needs
    /// to be `static` (i.e., pinned in memory) to prevent
    /// undefined behaviour as the lwip callbacks will expect it to
    /// be pinned in memory which static provides.
    fn as_mut_ptr(&self) -> *mut c_void {
        self as *const _ as *mut c_void
    }

    /// Binds a `UDPSocket<N>` to a port to receive data on.
    pub async fn bind(&'static self, port: u16) -> Result<(), err_t> {
        // SAFETY: runs inside `async_lwip` (core lock held), as required by
        // `udp_new`/`udp_bind`/`udp_recv`/`udp_remove`. `self` is
        // `&'static`, so `self.as_mut_ptr()` registered as the pcb's `recv`
        // arg stays valid for as long as the pcb (and therefore this
        // `UdpSocket`) exists.
        super::async_lwip(|| unsafe {
            let pcb = udp_new();
            if pcb.is_null() {
                return Err(err_t::Mem);
            }
            let err = udp_bind(pcb, &ip_addr_any, port);
            if err == err_t::Ok {
                udp_recv(pcb, Some(Self::_recv), self.as_mut_ptr());
                self.pcb.store(pcb, Ordering::Release);
                Ok(())
            } else {
                udp_remove(pcb);
                Err(err)
            }
        })
        .await
    }

    /// Broadcast a `PacketBuffer` across UDP.
    pub async fn broadcast(&self, mut pbuf: PacketBuffer, port: u16) -> Result<(), err_t> {
        // SAFETY: runs inside `async_lwip` (core lock held); `pcb` is
        // checked non-null above, and `pbuf.as_mut_ptr()` is a live,
        // exclusively-owned pbuf (owned by the `PacketBuffer` we hold by
        // value) that `udp_sendto` consumes/references only for the
        // duration of the call.
        super::async_lwip(|| unsafe {
            let pcb = self.pcb.load(Ordering::Acquire);
            if pcb.is_null() {
                return Err(err_t::Val);
            }
            let addr: ip_addr_t = ip_addr_t { addr: u32::MAX };
            let err = udp_sendto(pcb, pbuf.as_mut_ptr(), &addr, port);
            if err == err_t::Ok {
                Ok(())
            } else {
                log_error!("{err:?}");
                Err(err)
            }
        })
        .await
    }

    /// Handle a packet the has been received in the UDP port
    pub async fn receive(&'static self) -> (Ipv4Addr, PacketBuffer) {
        self.channel.receive().await
    }

    /// The callback handler for receiving UDP packets.
    ///
    /// # Safety
    /// Called by lwIP as a `udp_recv_fn`; `arg` must be either null or the
    /// `*mut c_void` registered via `udp_recv` in `bind()`, i.e. a valid
    /// `*const UdpSocket<N>`. `pbuf`, if non-null, is a pbuf this callback
    /// takes ownership of. `addr` must be a valid, non-null pointer to an
    /// `ip_addr_t` for the duration of the call (lwIP always supplies one
    /// for UDP receive callbacks).
    unsafe extern "C" fn _recv(
        arg: *mut c_void,
        _pcb: *mut pcb,
        pbuf: *mut pbuf,
        addr: *const ip_addr_t,
        _port: u16,
    ) {
        if arg.is_null() {
            return;
        }

        // SAFETY: `arg` is valid per this function's Safety contract.
        let sock = unsafe { &*(arg as *const UdpSocket<N>) };

        let Ok(pb) = PacketBuffer::try_from(pbuf) else {
            // Should only fail if the pbuf is null
            return;
        };

        // SAFETY: `addr` is non-null and valid per this function's Safety
        // contract (lwIP-provided for the duration of this call).
        // Lwip - network byte order Big-Endian. Host ARM expecting Little-Endian.
        let addr = unsafe { Ipv4Addr::from_bits(u32::from_be((*addr).addr)) };

        let _ = sock.channel.try_send((addr, pb));
        // If we fail to send the PacketBuffer then it
        // will free itself when dropped
    }
}

impl<const N: usize> Drop for UdpSocket<N> {
    fn drop(&mut self) {
        // SAFETY: `blocking_lwip` ensures `lock_tcpip_core` is held for
        // these udp_* calls. Note, unlike `TcpListener`/`TcpConnection`'s
        // `Drop` impls, `pcb` is *not* checked for null here before being
        // passed to `udp_recv`/`udp_remove` — see review notes for the
        // scenario where this can be a null pcb.
        let _ = super::blocking_lwip(|| unsafe {
            let pcb = self.pcb.swap(core::ptr::null_mut(), Ordering::AcqRel);
            if !pcb.is_null() {
                udp_recv(pcb, None, core::ptr::null_mut());
                udp_remove(pcb);
            }
            Ok(())
        });
    }
}
