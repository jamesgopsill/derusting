use core::{ffi::c_void, sync::atomic::Ordering};

use embassy_sync::{
    blocking_mutex::{Mutex, raw::ThreadModeRawMutex},
    channel::Channel,
};
use portable_atomic::AtomicPtr;

use crate::{
    log_info,
    lwip::{
        self, bindings::*, core::LwipCore, ipaddr::IpAddr, packet_buffer::ZeroCopyPacketBuffer,
    },
};

pub struct UdpProtocolControlBlock {
    inner: *mut lwip_pcb,
}

impl UdpProtocolControlBlock {}

unsafe impl Send for UdpProtocolControlBlock {}

impl TryFrom<*mut lwip_pcb> for UdpProtocolControlBlock {
    type Error = ();
    fn try_from(value: *mut lwip_pcb) -> Result<Self, ()> {
        if value.is_null() {
            Err(())
        } else {
            Ok(Self { inner: value })
        }
    }
}
impl TryFrom<&AtomicPtr<lwip_pcb>> for UdpProtocolControlBlock {
    type Error = ();
    fn try_from(value: &AtomicPtr<lwip_pcb>) -> Result<Self, ()> {
        let value = value.load(Ordering::SeqCst);
        Self::try_from(value)
    }
}

impl UdpProtocolControlBlock {
    pub fn as_mut_ptr(&self) -> *mut lwip_pcb {
        self.inner
    }

    pub fn bind(&self, port: u16, _core: &LwipCore) -> Result<(), LwipError> {
        let err = unsafe { udp_bind(self.as_mut_ptr(), &ip_addr_any, port) };
        err.into()
    }

    pub fn recv(&self, sock: &'static UdpSocket, _core: &LwipCore) {
        unsafe {
            udp_recv(
                self.as_mut_ptr(),
                Some(on_udp_recv),
                sock as *const _ as *mut c_void,
            )
        };
    }

    pub fn broadcast(
        &self,
        mut pbuf: ZeroCopyPacketBuffer,
        port: u16,
        _core: &LwipCore,
    ) -> Result<(), LwipError> {
        let addr: lwip_ipaddr = lwip_ipaddr { addr: u32::MAX };
        let err = unsafe { udp_sendto(self.as_mut_ptr(), pbuf.as_mut_ptr(), &addr, port) };
        log_info!("Broadcast result: {:?}", err);
        err.into()
    }

    pub fn new(_core: &LwipCore) -> Result<UdpProtocolControlBlock, ()> {
        let pcb = unsafe { udp_new() };
        UdpProtocolControlBlock::try_from(pcb)
    }

    pub fn remove(self, _core: &LwipCore) {
        unsafe { udp_remove(self.as_mut_ptr()) };
    }
}

pub struct UdpSocket {
    pub packets: Channel<ThreadModeRawMutex, (IpAddr, ZeroCopyPacketBuffer), 5>,
    pub pcb: Mutex<ThreadModeRawMutex, UdpProtocolControlBlock>,
}

impl UdpSocket {
    pub fn new(pcb: UdpProtocolControlBlock) -> Self {
        Self {
            packets: Channel::new(),
            pcb: Mutex::new(pcb),
        }
    }

    pub fn broadcast(&self, pbuf: ZeroCopyPacketBuffer, port: u16) -> Result<(), LwipError> {
        log_info!("Socket broadcasting");
        lwip::core::with_lwip_core(|core| self.pcb.lock(|pcb| pcb.broadcast(pbuf, port, &core)))
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

    let Ok(pb) = ZeroCopyPacketBuffer::try_from(pbuf) else {
        return;
    };

    let Ok(addr) = IpAddr::try_from(addr) else {
        return;
    };

    // NOTE. may have to handle missed sends to clean them up.
    let _ = socket.packets.try_send((addr, pb));
}
