use core::net::Ipv4Addr;

use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};
use embassy_time::Instant;
use heapless::LinearMap;

use crate::{lwip::my_ipaddr, tasks::messages::Ledger};

pub type AddressBook<const N: usize> = Mutex<ThreadModeRawMutex, LinearMap<Ipv4Addr, Instant, N>>;

pub struct LedgerState {
    pub ledger: Ledger,
    pub received: Instant,
}

impl LedgerState {
    pub fn new(ledger: Ledger) -> Self {
        Self {
            ledger,
            received: Instant::now(),
        }
    }

    pub fn is_owner(&self) -> bool {
        let Some(my_addr) = my_ipaddr() else {
            return false;
        };
        my_addr == self.ledger.owner
    }
}

pub type JobLedger = Mutex<ThreadModeRawMutex, Option<LedgerState>>;
