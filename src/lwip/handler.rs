use core::{ops::DerefMut, ptr};

use alloc::vec::Vec;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};

use crate::{
    log_info,
    lwip::{self, tcp::TcpProtocolControlBlock},
};

type Type = Vec<u8>;

pub struct Handler {
    pub channel: Channel<CriticalSectionRawMutex, (usize, [u8; 1024]), 5>,
    pub request_buf: Vec<u8>,
    pub tcp: TcpProtocolControlBlock,
    pub closed: bool,
}

impl Handler {
    pub fn new(tcp: TcpProtocolControlBlock) -> Self {
        Self {
            channel: Channel::new(),
            request_buf: Vec::new(),
            tcp,
            closed: false,
        }
    }

    pub fn close(self) {
        lwip::core::with_lwip_core(|core| {
            let _ = self.tcp.output();
            self.tcp.recv(None);
            self.tcp.err(None);
            self.tcp.arg(ptr::null_mut());
        });
    }

    pub fn write(&self, bytes: &[u8]) {
        lwip::core::with_lwip_core(|core| {
            self.tcp.write_with_core(bytes, &core);
        });
    }

    pub fn write_and_close(self, bytes: &[u8]) {
        self.write(bytes);
        self.close();
    }
}
