use core::{ffi::c_void, marker::PhantomPinned, net::Ipv4Addr, pin::Pin};

use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel, signal::Signal};

use crate::{
    log_error, log_info,
    lwip::{bindings::*, packet_buffer::PacketBuffer},
};

pub struct UdpSocket<const N: usize> {
    pcb: *mut lwip_pcb,
    channel: Channel<ThreadModeRawMutex, (Ipv4Addr, PacketBuffer), N>,
    port: u16,
    _pin: PhantomPinned,
}

impl<const N: usize> UdpSocket<N> {
    pub fn new() -> Self {
        Self {
            pcb: core::ptr::null_mut(),
            channel: Channel::new(),
            port: 9090,
            _pin: PhantomPinned,
        }
    }

    pub async fn listen(self: Pin<&mut Self>, port: u16) -> Result<(), ()> {
        let this = unsafe { self.get_unchecked_mut() };
        this.port = port;
        let this_ptr = this as *mut _ as *mut c_void;
        let mut signal: Signal<ThreadModeRawMutex, bool> = Signal::new();
        let signal_ptr = &mut signal as *mut _ as *mut c_void;
        let mut tuple = (this_ptr, signal_ptr);
        let ctx = &mut tuple as *mut _ as *mut c_void;
        unsafe { tcpip_callback(Self::_listen, ctx) };
        let res = signal.wait().await;
        match res {
            true => Ok(()),
            false => Err(()),
        }
    }

    unsafe extern "C" fn _listen(ctx: *mut c_void) {
        unsafe {
            if ctx.is_null() {
                return;
            }
            let (udp_ptr, signal_ptr) =
                *(ctx as *const (*mut Self, *mut Signal<ThreadModeRawMutex, bool>));

            let udp: &mut Self = &mut *udp_ptr;
            let signal: &mut Signal<ThreadModeRawMutex, bool> = &mut *signal_ptr;

            if !udp.pcb.is_null() {
                // PCB already exists
                // TODO: close and reallocate is a future
                // possibility.
                signal.signal(false);
                return;
            }
            log_info!("New UDP");
            udp.pcb = udp_new();
            let err = udp_bind(udp.pcb, &ip_addr_any, udp.port);
            if err != LwipError::Ok {
                log_info!("Binding error");
                udp_remove(udp.pcb);
                udp.pcb = core::ptr::null_mut();
                signal.signal(false);
            } else {
                log_info!("Binding success");
                udp_recv(udp.pcb, Some(Self::recv), udp_ptr as *mut c_void);
                signal.signal(true);
            }
        }
    }

    /*
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
    */

    pub async fn broadcast(self: Pin<&Self>, mut pbuf: PacketBuffer) -> Result<(), ()> {
        if self.pcb.is_null() {
            return Err(());
        }
        let pcb_ptr = self.pcb;
        let mut signal: Signal<ThreadModeRawMutex, bool> = Signal::new();
        let signal_ptr = &mut signal as *mut _ as *mut c_void;
        let pbuf_ptr = pbuf.as_mut_ptr();
        let mut tuple = (pcb_ptr, signal_ptr, pbuf_ptr, self.port);
        let ctx = &mut tuple as *mut _ as *mut c_void;
        unsafe { tcpip_callback(Self::_broadcast, ctx) };
        let res = signal.wait().await;
        log_info!("Sent: {res}");
        match res {
            true => Ok(()),
            false => Err(()),
        }

        /*
        sys_mutex_lock(&raw mut lock_tcpip_core);
        let addr: lwip_ipaddr = lwip_ipaddr { addr: u32::MAX };
        let err = udp_sendto(self.pcb, pbuf.as_mut_ptr(), &addr, port);
        sys_mutex_unlock(&raw mut lock_tcpip_core);
        err.into()
        */
    }

    unsafe extern "C" fn _broadcast(ctx: *mut c_void) {
        unsafe {
            if ctx.is_null() {
                return;
            }
            // Unpack the tuple
            let (pcb, signal_ptr, pbuf, port) = *(ctx as *const (
                *mut lwip_pcb,
                *mut Signal<ThreadModeRawMutex, bool>,
                *mut lwip_pbuf,
                u16,
            ));
            // Reconstruct them
            let signal: &mut Signal<ThreadModeRawMutex, bool> = &mut *signal_ptr;

            if pcb.is_null() {
                log_error!("PCB is null");
                signal.signal(false);
                return;
            }
            let addr: lwip_ipaddr = lwip_ipaddr { addr: u32::MAX };
            let err = udp_sendto(pcb, pbuf, &addr, port);
            if err == LwipError::Ok {
                signal.signal(true);
            } else {
                log_error!("{err:?}");
                signal.signal(false);
            }
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
