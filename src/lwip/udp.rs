use core::{ffi::c_void, marker::PhantomPinned, net::Ipv4Addr, pin::Pin};

use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};

use crate::lwip::{bindings::*, packet_buffer::PacketBuffer};

pub struct UdpSocket<const N: usize> {
    pcb: *mut lwip_pcb,
    channel: Channel<ThreadModeRawMutex, (Ipv4Addr, PacketBuffer), N>,
    _pin: PhantomPinned,
}

impl<const N: usize> UdpSocket<N> {
    pub fn new() -> Self {
        Self {
            pcb: core::ptr::null_mut(),
            channel: Channel::new(),
            _pin: PhantomPinned,
        }
    }

    pub fn listen(self: Pin<&mut Self>, port: u16) -> Result<(), ()> {
        unsafe {
            // Safety: We do not move `*this` out of memory.
            let this = self.get_unchecked_mut();
            sys_mutex_lock(&raw mut lock_tcpip_core);
            this.pcb = udp_new();
            let err = udp_bind(this.pcb, &ip_addr_any, port);
            if err != LwipError::Ok {
                udp_remove(this.pcb);
                sys_mutex_unlock(&raw mut lock_tcpip_core);
                return Err(());
            }
            let ctx_ptr = this as *mut Self as *mut c_void;
            udp_recv(this.pcb, Some(Self::recv), ctx_ptr);
            sys_mutex_unlock(&raw mut lock_tcpip_core);
            Ok(())
        }
    }

    pub fn broadcast(self: Pin<&Self>, mut pbuf: PacketBuffer, port: u16) -> Result<(), LwipError> {
        if self.pcb.is_null() {
            return Err(LwipError::Arg);
        }
        unsafe {
            sys_mutex_lock(&raw mut lock_tcpip_core);
            let addr: lwip_ipaddr = lwip_ipaddr { addr: u32::MAX };
            let err = udp_sendto(self.pcb, pbuf.as_mut_ptr(), &addr, port);
            sys_mutex_unlock(&raw mut lock_tcpip_core);
            err.into()
        }
    }

    pub async fn with_packet<F>(self: Pin<&Self>, fcn: F)
    where
        F: AsyncFnOnce((Ipv4Addr, PacketBuffer)),
    {
        let packet = self.channel.receive().await;
        fcn(packet).await;
    }

    unsafe extern "C" fn recv(
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
            udp_recv(self.pcb, None, core::ptr::null_mut());
            udp_remove(self.pcb);
            sys_mutex_unlock(&raw mut lock_tcpip_core);
        }
    }
}
