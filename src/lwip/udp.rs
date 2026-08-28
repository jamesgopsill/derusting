use core::{ffi::c_void, sync::atomic::Ordering};

use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Channel, TrySendError},
};
use portable_atomic::AtomicPtr;

use crate::{
    log_info,
    lwip::{
        bindings::*,
        core::LwipCore,
        ipaddr::IpAddr,
        packet_buffer::{PacketBuffer, UdpPacket},
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

    pub fn recv(&self, channel: &'static UdpChannel, _core: &LwipCore) {
        unsafe {
            udp_recv(
                self.as_mut_ptr(),
                Some(on_udp_recv),
                channel as *const _ as *mut c_void,
            )
        };
    }

    pub fn sendto(&self, pbuf: PacketBuffer, port: u16, _core: &LwipCore) -> Result<(), LwipError> {
        log_info!("udp_sendto");
        let addr: lwip_ipaddr = lwip_ipaddr { addr: u32::MAX };
        let err = unsafe { udp_sendto(self.as_mut_ptr(), pbuf.as_mut_ptr(), &addr, port) };
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

pub struct UdpDatagram {
    pub from: IpAddr,
    pub packet: UdpPacket,
}

pub struct UdpChannel {
    inner: Channel<CriticalSectionRawMutex, UdpDatagram, 5>,
}

impl Default for UdpChannel {
    fn default() -> Self {
        Self {
            inner: Channel::new(),
        }
    }
}

impl UdpChannel {
    pub fn try_send(&self, msg: UdpDatagram) -> Result<(), TrySendError<UdpDatagram>> {
        self.inner.try_send(msg)
    }

    pub async fn receive(&self) -> UdpDatagram {
        self.inner.receive().await
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
    let channel = unsafe { &*(arg as *const UdpChannel) };

    let Ok(pb) = PacketBuffer::try_from(pbuf) else {
        return;
    };

    let Ok(addr) = IpAddr::try_from(addr) else {
        return;
    };

    let msg = UdpDatagram {
        from: addr,
        packet: pb.into_udp_packet(),
    };

    // Do not hold up the callback. We may drop.
    channel.try_send(msg);
}
