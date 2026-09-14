use core::{
    ffi::c_void,
    net::Ipv4Addr,
    sync::atomic::{AtomicPtr, Ordering},
};

use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, signal::Signal,
};

use crate::{
    log_error,
    lwip::{bindings::*, packet_buffer::PacketBuffer},
};

struct ListenContext {
    port: u16,
    socket_ptr: *mut c_void,
    signal: Signal<CriticalSectionRawMutex, Result<*mut lwip_pcb, LwipError>>,
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
    signal: Signal<CriticalSectionRawMutex, Result<(), LwipError>>,
}

impl BroadcastContext {
    fn as_mut_ptr(&mut self) -> *mut c_void {
        self as *mut _ as *mut c_void
    }
}

pub struct UdpSocket<const N: usize> {
    pcb: AtomicPtr<lwip_pcb>,
    channel: Channel<CriticalSectionRawMutex, (Ipv4Addr, PacketBuffer), N>,
}

unsafe impl<const N: usize> Sync for UdpSocket<N> {}
unsafe impl<const N: usize> Send for UdpSocket<N> {}

impl<const N: usize> UdpSocket<N> {
    pub fn new() -> Self {
        Self {
            pcb: AtomicPtr::new(core::ptr::null_mut()),
            channel: Channel::new(),
        }
    }

    fn as_mut_ptr(&'static self) -> *mut c_void {
        self as *const _ as *mut c_void
    }

    pub async fn bind(&'static self, port: u16) -> Result<(), LwipError> {
        let ctx = ListenContext {
            port,
            socket_ptr: self.as_mut_ptr(),
            signal: Signal::new(),
        };
        let mut ctx = core::pin::pin!(ctx);
        let err = unsafe { tcpip_callback(Self::_bind, ctx.as_mut_ptr()) };
        if err != LwipError::Ok {
            return Err(err);
        }
        let pcb = ctx.signal.wait().await?;
        self.pcb.store(pcb, Ordering::Release);
        Ok(())
    }

    unsafe extern "C" fn _bind(ctx: *mut c_void) {
        unsafe {
            if ctx.is_null() {
                return;
            }
            let ctx = &mut *(ctx as *mut ListenContext);
            let pcb = udp_new();
            if pcb.is_null() {
                ctx.signal.signal(Err(LwipError::Mem));
                return;
            }
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

    pub async fn broadcast(&'static self, pbuf: PacketBuffer, port: u16) -> Result<(), LwipError> {
        let pcb = self.pcb.load(Ordering::Acquire);
        if pcb.is_null() {
            return Err(LwipError::Val);
        }
        let ctx = BroadcastContext {
            pcb,
            port,
            pbuf,
            signal: Signal::new(),
        };
        let mut ctx = core::pin::pin!(ctx);
        let err = unsafe { tcpip_callback(Self::_broadcast, ctx.as_mut_ptr()) };
        if err != LwipError::Ok {
            return Err(err);
        }
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

    pub async fn with_packet<F>(&'static self, fcn: F)
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
            unsafe { pbuf_free(pbuf) };
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
