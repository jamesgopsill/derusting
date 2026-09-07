use core::{cell::RefCell, ffi::c_void, net::Ipv4Addr};

use embassy_sync::{
    blocking_mutex::{Mutex, raw::ThreadModeRawMutex},
    channel::Channel,
};

use crate::{
    log_info,
    lwip::{bindings::*, packet_buffer::PacketBuffer},
};

/// A UDP control block can be interacted within
/// or out of the thread.
pub trait Mode {}

pub struct InThread;
impl Mode for InThread {}

pub struct OutThread;
impl Mode for OutThread {}

pub struct UdpProtocolControlBlock<T: Mode> {
    inner: *mut lwip_pcb,
    _mode: T,
}

impl TryFrom<*mut lwip_pcb> for UdpProtocolControlBlock<InThread> {
    type Error = ();
    fn try_from(value: *mut lwip_pcb) -> Result<Self, ()> {
        if value.is_null() {
            Err(())
        } else {
            Ok(Self {
                inner: value,
                _mode: InThread,
            })
        }
    }
}

impl TryFrom<*mut lwip_pcb> for UdpProtocolControlBlock<OutThread> {
    type Error = ();
    fn try_from(value: *mut lwip_pcb) -> Result<Self, ()> {
        if value.is_null() {
            Err(())
        } else {
            Ok(Self {
                inner: value,
                _mode: OutThread,
            })
        }
    }
}

impl From<UdpProtocolControlBlock<InThread>> for UdpProtocolControlBlock<OutThread> {
    fn from(value: UdpProtocolControlBlock<InThread>) -> Self {
        UdpProtocolControlBlock {
            inner: value.inner,
            _mode: OutThread,
        }
    }
}

impl<T: Mode> UdpProtocolControlBlock<T> {
    pub fn as_mut_ptr(&self) -> *mut lwip_pcb {
        self.inner
    }
}

impl UdpProtocolControlBlock<InThread> {
    pub fn bind(&self, port: u16) -> Result<(), LwipError> {
        let err = unsafe { udp_bind(self.as_mut_ptr(), &ip_addr_any, port) };
        err.into()
    }

    pub fn recv(&self, sock: &'static UdpSocket) {
        log_info!("setting up udp_recv()");
        unsafe {
            udp_recv(
                self.as_mut_ptr(),
                Some(on_udp_recv),
                sock as *const _ as *mut c_void,
            )
        };
    }

    pub fn broadcast(&self, mut pbuf: PacketBuffer, port: u16) -> Result<(), LwipError> {
        let addr: lwip_ipaddr = lwip_ipaddr { addr: u32::MAX };
        let err = unsafe { udp_sendto(self.as_mut_ptr(), pbuf.as_mut_ptr(), &addr, port) };
        err.into()
    }

    #[allow(unused)]
    pub fn remove(self) {
        unsafe { udp_remove(self.as_mut_ptr()) };
    }
}

impl UdpProtocolControlBlock<OutThread> {
    pub fn new() -> Result<Self, ()> {
        unsafe { sys_mutex_lock(&raw mut lock_tcpip_core) };
        let pcb = unsafe { tcp_new() };
        unsafe { sys_mutex_unlock(&raw mut lock_tcpip_core) };
        Self::try_from(pcb)
    }

    pub fn with_core<R>(
        &mut self,
        fcn: impl FnOnce(&mut UdpProtocolControlBlock<InThread>) -> R,
    ) -> R {
        let pcb = unsafe { &mut *(self as *mut Self as *mut UdpProtocolControlBlock<InThread>) };
        unsafe { sys_mutex_lock(&raw mut lock_tcpip_core) };
        let res = fcn(pcb);
        unsafe { sys_mutex_unlock(&raw mut lock_tcpip_core) };
        res
    }
}

pub struct UdpSocket {
    pub packets: Channel<ThreadModeRawMutex, (Ipv4Addr, PacketBuffer), 5>,
    pub pcb: Mutex<ThreadModeRawMutex, RefCell<UdpProtocolControlBlock<OutThread>>>,
}

impl UdpSocket {
    pub fn new(pcb: UdpProtocolControlBlock<OutThread>) -> Self {
        Self {
            packets: Channel::new(),
            pcb: Mutex::new(RefCell::new(pcb)),
        }
    }

    pub fn broadcast(&self, pbuf: PacketBuffer, port: u16) -> Result<(), LwipError> {
        // log_info!("Socket broadcasting");
        self.pcb.lock(|rc| {
            let mut pcb = rc.borrow_mut();
            pcb.with_core(|pcb| pcb.broadcast(pbuf, port))
        })
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn on_udp_recv(
    arg: *mut c_void,
    _pcb: *mut lwip_pcb,
    pbuf: *mut lwip_pbuf,
    addr: *const lwip_ipaddr,
    _port: u16,
) {
    if arg.is_null() {
        return;
    }
    let socket = unsafe { &*(arg as *const UdpSocket) };

    let Ok(pb) = PacketBuffer::try_from(pbuf) else {
        return;
    };

    // Lwip - network byte order Big-Endian. Host ARM expecting Little-Endian.
    let addr = unsafe { Ipv4Addr::from_bits(u32::from_be((*addr).addr)) };

    // NOTE. may have to handle missed sends to clean them up.
    let _ = socket.packets.try_send((addr, pb));
}
