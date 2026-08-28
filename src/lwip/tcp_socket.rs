use core::cell::RefCell;

use embassy_sync::{
    blocking_mutex::{Mutex, raw::CriticalSectionRawMutex},
    channel::Channel,
};

use crate::lwip::packet_buffer::ZeroCopyPacketBuffer;
use crate::{
    log_error, log_info,
    lwip::{self, core::LwipCore, tcp::TcpProtocolControlBlock},
};

pub struct TcpSocket {
    pub packets: Channel<CriticalSectionRawMutex, Option<ZeroCopyPacketBuffer>, 2>,
    pub pcb: Mutex<CriticalSectionRawMutex, RefCell<Option<TcpProtocolControlBlock>>>,
}

impl<'a> Default for TcpSocket {
    fn default() -> Self {
        Self {
            packets: Channel::new(),
            pcb: Mutex::new(RefCell::new(None)),
        }
    }
}

impl TcpSocket {
    pub fn on_accept_add_pcb(&self, pcb: TcpProtocolControlBlock) {
        self.pcb.lock(|rc| *rc.borrow_mut() = Some(pcb))
    }

    pub fn close(&self) {
        log_info!("Handler: closing TCP");
        self.pcb.lock(|rc| {
            if let Some(pcb) = rc.take() {
                pcb.recv(None);
                pcb.err(None);
                pcb.sent(None);
                pcb.accept(None);
                let _ = pcb.output();
                if pcb.close().is_err() {
                    log_error!("Error Closing");
                    // TODO: Abort
                }
            }
        });
        self.packets.clear();
        let _ = self.packets.try_send(None);
    }

    pub fn close_with_core(&self, _core: &LwipCore) {
        self.close();
    }

    pub fn write_and_close(&self, bytes: &[u8]) {
        lwip::core::with_lwip_core(|core| {
            self.pcb.lock(|rc| {
                if let Some(pcb) = &*rc.borrow() {
                    let _ = pcb.write_with_core(bytes, &core);
                }
            });
            self.close_with_core(&core);
        });
    }

    pub fn recevd_in_lwip_thread(&self, len: u16) {
        self.pcb.lock(|rc| {
            if let Some(pcb) = &*rc.borrow() {
                pcb.recved(len);
            }
        });
    }
}
