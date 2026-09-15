use core::{
    ffi::c_void,
    net::Ipv4Addr,
    sync::atomic::{AtomicPtr, Ordering},
};

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};

use crate::{
    log_error,
    lwip::{bindings::*, execute_in_tcpip_thread, packet_buffer::PacketBuffer},
};

pub struct UdpSocket<const N: usize> {
    pcb: AtomicPtr<lwip_pcb>,
    channel: Channel<CriticalSectionRawMutex, (Ipv4Addr, PacketBuffer), N>,
}

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
    pub async fn bind(&'static self, port: u16) -> Result<(), LwipError> {
        execute_in_tcpip_thread(|| unsafe {
            let pcb = udp_new();
            if pcb.is_null() {
                return Err(LwipError::Mem);
            }
            let err = udp_bind(pcb, &ip_addr_any, port);
            if err == LwipError::Ok {
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
    pub async fn broadcast(&self, mut pbuf: PacketBuffer, port: u16) -> Result<(), LwipError> {
        execute_in_tcpip_thread(|| unsafe {
            let pcb = self.pcb.load(Ordering::Acquire);
            if pcb.is_null() {
                return Err(LwipError::Val);
            }
            let addr: lwip_ipaddr = lwip_ipaddr { addr: u32::MAX };
            let err = udp_sendto(pcb, pbuf.as_mut_ptr(), &addr, port);
            if err == LwipError::Ok {
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

    /// The callback handler for receiving UPD packets.
    unsafe extern "C" fn _recv(
        arg: *mut c_void,
        _pcb: *mut lwip_pcb,
        pbuf: *mut lwip_pbuf,
        addr: *const lwip_ipaddr,
        _port: u16,
    ) {
        if arg.is_null() {
            return;
        }

        let sock = unsafe { &*(arg as *const UdpSocket<N>) };

        let Ok(pb) = PacketBuffer::try_from(pbuf) else {
            // Should only fail if the pbuf is null
            return;
        };

        // Lwip - network byte order Big-Endian. Host ARM expecting Little-Endian.
        let addr = unsafe { Ipv4Addr::from_bits(u32::from_be((*addr).addr)) };

        let _ = sock.channel.try_send((addr, pb));
        // If we fail to send the PacketBuffer then it
        // will free itself when dropped
    }
}

impl<const N: usize> Drop for UdpSocket<N> {
    fn drop(&mut self) {
        unsafe {
            sys_mutex_lock(&raw mut lock_tcpip_core);
            let pcb = self.pcb.swap(core::ptr::null_mut(), Ordering::AcqRel);
            udp_recv(pcb, None, core::ptr::null_mut());
            udp_remove(pcb);
            sys_mutex_unlock(&raw mut lock_tcpip_core);
        }
    }
}
