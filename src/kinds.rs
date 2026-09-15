use core::net::Ipv4Addr;

use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};
use embassy_time::Instant;
use heapless::LinearMap;

use crate::tasks::messages::Ledger;

pub type AddressBook<const N: usize> =
    Mutex<ThreadModeRawMutex, LinearMap<Ipv4Addr, (Instant, bool), N>>;
pub type JobLedger = Mutex<ThreadModeRawMutex, Option<Ledger>>;
