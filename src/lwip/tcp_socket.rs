use core::cell::RefCell;

use embassy_sync::{
    blocking_mutex::{Mutex, raw::CriticalSectionRawMutex},
    channel::Channel,
};

use crate::lwip::{
    packet_buffer::ZeroCopyPacketBuffer,
    tcp::{InThreadTcpProtocolControlBlock, OutThreadTcpProtocolControlBlock},
};
use crate::{
    log_error,
    lwip::{self},
};

pub struct TcpSocket {
    pub packets: Channel<CriticalSectionRawMutex, Option<ZeroCopyPacketBuffer>, 10>,
    pub pcb: Mutex<CriticalSectionRawMutex, RefCell<Option<OutThreadTcpProtocolControlBlock>>>,
}

impl Default for TcpSocket {
    fn default() -> Self {
        Self {
            packets: Channel::new(),
            pcb: Mutex::new(RefCell::new(None)),
        }
    }
}

impl TcpSocket {
    // Thread side function
    pub fn on_accept_add_pcb(&self, pcb: InThreadTcpProtocolControlBlock) {
        let pcb = OutThreadTcpProtocolControlBlock::from(pcb);
        self.pcb.lock(|rc| *rc.borrow_mut() = Some(pcb))
    }

    // Out of thread function
    pub fn reset(&self) {
        lwip::core::with_lwip_core(|core| {
            self.pcb.lock(|rc| {
                if let Some(pcb) = rc.take() {
                    pcb.recv(None, &core);
                    pcb.err(None, &core);
                    pcb.sent(None, &core);
                    pcb.accept(None, &core);
                    let _ = pcb.output(&core);
                    if pcb.close(&core).is_err() {
                        log_error!("Error Closing");
                    }
                }
            });
        });
        self.packets.clear();
    }

    // Out of thread function
    pub fn send_response(&self, bytes: &[u8]) {
        lwip::core::with_lwip_core(|core| {
            self.pcb.lock(|rc| {
                if let Some(pcb) = &*rc.borrow() {
                    let _ = pcb.write(bytes, &core);
                }
            });
        });
        self.reset();
    }
}
