use core::{ffi::c_void, marker::PhantomPinned, net::Ipv4Addr, pin::Pin};

use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel, signal::Signal};

use crate::{
    log_error,
    lwip::{bindings::*, packet_buffer::PacketBuffer},
};

struct ListenContext {
    port: u16,
    socket_ptr: *mut c_void,
    signal: Signal<ThreadModeRawMutex, Result<*mut lwip_pcb, LwipError>>,
}

impl ListenContext {
    fn as_mut_ptr(&mut self) -> *mut c_void {
        self as *mut _ as *mut c_void
    }
}

struct BroadcastContext {
    pcb: *mut lwip_pcb,
    port: u16,
    pbuf: PacketBuffer,
    signal: Signal<ThreadModeRawMutex, Result<(), LwipError>>,
}

impl BroadcastContext {
    fn as_mut_ptr(&mut self) -> *mut c_void {
        self as *mut _ as *mut c_void
    }
}

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

    pub fn as_mut_ptr(&mut self) -> *mut c_void {
        self as *mut _ as *mut c_void
    }

    pub async fn listen(self: Pin<&mut Self>, port: u16) -> Result<(), LwipError> {
        let this = unsafe { self.get_unchecked_mut() };
        let mut ctx = ListenContext {
            port,
            socket_ptr: this.as_mut_ptr(),
            signal: Signal::new(),
        };
        unsafe { tcpip_callback(Self::_listen, ctx.as_mut_ptr()) };
        let pcb = ctx.signal.wait().await?;
        this.pcb = pcb;
        Ok(())
    }

    unsafe extern "C" fn _listen(ctx: *mut c_void) {
        unsafe {
            if ctx.is_null() {
                return;
            }
            let ctx = &mut *(ctx as *mut ListenContext);
            let pcb = udp_new();
            let err = udp_bind(pcb, &ip_addr_any, ctx.port);
            if err != LwipError::Ok {
                udp_remove(pcb);
                ctx.signal.signal(Err(err));
            } else {
                udp_recv(pcb, Some(Self::recv), ctx.socket_ptr);
                ctx.signal.signal(Ok(pcb));
            }
        }
    }

    pub async fn broadcast(
        self: Pin<&Self>,
        pbuf: PacketBuffer,
        port: u16,
    ) -> Result<(), LwipError> {
        if self.pcb.is_null() {
            return Err(LwipError::Val);
        }
        let mut ctx = BroadcastContext {
            pcb: self.pcb,
            port,
            pbuf,
            signal: Signal::new(),
        };
        unsafe { tcpip_callback(Self::_broadcast, ctx.as_mut_ptr()) };
        ctx.signal.wait().await
    }

    unsafe extern "C" fn _broadcast(ctx: *mut c_void) {
        unsafe {
            if ctx.is_null() {
                return;
            }
            let ctx = &mut *(ctx as *mut BroadcastContext);
            if ctx.pcb.is_null() {
                log_error!("PCB is null");
                ctx.signal.signal(Err(LwipError::Val));
                return;
            }
            let addr: lwip_ipaddr = lwip_ipaddr { addr: u32::MAX };
            let err = udp_sendto(ctx.pcb, ctx.pbuf.as_mut_ptr(), &addr, ctx.port);
            if err == LwipError::Ok {
                ctx.signal.signal(Ok(()));
            } else {
                log_error!("{err:?}");
                ctx.signal.signal(Err(err));
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
