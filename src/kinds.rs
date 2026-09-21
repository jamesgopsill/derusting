use core::net::Ipv4Addr;

use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};
use embassy_time::Instant;
use heapless::LinearMap;

use crate::{lwip::my_ipaddr, tasks::messages::Ledger};

/// Tracks the other machines seen on the network, keyed by IP address, with
/// the `Instant` each was last heard from.
pub type AddressBook<const N: usize> = Mutex<ThreadModeRawMutex, LinearMap<Ipv4Addr, Instant, N>>;

/// A `Ledger` plus the time it was last received, used to detect a stale
/// ledger whose owner has gone offline.
pub struct LedgerState {
    pub ledger: Ledger,
    pub received: Instant,
}

impl LedgerState {
    /// Wraps a freshly received or created `Ledger`, timestamped as of now.
    pub fn new(ledger: Ledger) -> Self {
        Self {
            ledger,
            received: Instant::now(),
        }
    }

    /// Whether this machine is the current owner of the ledger.
    pub fn is_owner(&self) -> bool {
        let Some(my_addr) = my_ipaddr() else {
            return false;
        };
        my_addr == self.ledger.owner
    }
}

/// The shared, possibly-not-yet-created token-ring ledger for this machine.
pub type JobLedger = Mutex<ThreadModeRawMutex, Option<LedgerState>>;
