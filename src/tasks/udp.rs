use core::net::Ipv4Addr;

use alloc::format;
use embassy_time::{Duration, Instant, Timer};
use heapless::HistoryBuf;
use heapless::index_set::FnvIndexSet;
use uuid::Uuid;

use crate::fs::{self, ReadBytes, make_path};
use crate::kinds::{AddressBook, JobLedger, LedgerState};
use crate::lwip::my_ipaddr;
use crate::lwip::udp::UdpSocket;
use crate::tasks::messages::{Ledger, Message, Payload};
use crate::{ADDRESS_BOOK_ENTRIES, UDP_CHANNEL_SIZE, marlin};
use crate::{log_error, log_info};

/// This task broadcasts a heartbeat to the network to inform
/// other machines that this machine is alive and on the network.
#[embassy_executor::task(pool_size = 1)]
pub async fn heartbeat(udp: &'static UdpSocket<UDP_CHANNEL_SIZE>) {
    loop {
        if let Some(addr) = my_ipaddr() {
            log_info!("[{:?}] heartbeat()", addr);
        } else {
            log_info!("[Unknown] heartbeat()");
        }
        Message::send_heartbeat(udp).await;
        Timer::after_secs(2).await;
    }
}

/// This task will receive and handle messages being sent over UDP
/// on the network.
#[embassy_executor::task(pool_size = 1)]
pub async fn udp_receiver(
    udp: &'static UdpSocket<UDP_CHANNEL_SIZE>,
    address_book: &'static AddressBook<ADDRESS_BOOK_ENTRIES>,
    ledger: &'static JobLedger,
) {
    log_info!("Ready to receive UDP packets");
    // let mut file_transfer = FileTransferTask::new();
    // Holds the last 12 unique message idempotencies
    let mut history: HistoryBuf<Uuid, 24> = HistoryBuf::new();
    loop {
        let (addr, packet) = udp.receive().await;
        log_info!("Received packet from: {addr}");

        let Some(msg) = packet.into_iter().next() else {
            continue;
        };

        let Ok(msg) = postcard::from_bytes::<Message>(msg) else {
            log_error!("Packet Deserialization failed");
            continue;
        };

        if history.contains(&msg.idempotency) {
            // Already processed it recently
            continue;
        }
        history.write(msg.idempotency);

        {
            let mut book = address_book.lock().await;
            if let Err(e) = book.insert(addr, Instant::now()) {
                log_error!("Address book error: {e:?}");
            };
        }

        if let Payload::NewJob(data) = msg.payload {
            // At the moment, all add it but could only the
            // owner really needs to do it in this format.
            // But will be good if we need to assign a new owner
            // if the current one goes offline.
            {
                let mut guard = ledger.lock().await;
                if let Some(ledger) = guard.as_mut() {
                    let _ = ledger.ledger.jobs.insert(data.guid);
                }
            }
            continue;
        }

        // Update the ledger I have to maintain sync. Only the owner
        // should be sending this message out.
        if let Payload::Ledger(sent_ledger) = msg.payload {
            {
                let mut l = ledger.lock().await;
                *l = Some(LedgerState::new(sent_ledger));
            }
            continue;
        }
    }
}

/// This task periodically checks the address book and cleans up
/// any address have not heard from in a while.
#[embassy_executor::task(pool_size = 1)]
pub async fn address_book_lifetime_check(address_book: &'static AddressBook<ADDRESS_BOOK_ENTRIES>) {
    loop {
        Timer::after_secs(10).await;
        let mut book = address_book.lock().await;
        book.retain(|_k, instant| instant.elapsed().as_secs() < 20);
    }
}

/// This task manages the token-ring ledger that is passed between
/// machines. We only do something if we are the owner of the ledger.
#[embassy_executor::task(pool_size = 1)]
pub async fn manage_ledger(
    udp: &'static UdpSocket<UDP_CHANNEL_SIZE>,
    address_book: &'static AddressBook<ADDRESS_BOOK_ENTRIES>,
    ledger: &'static JobLedger,
) -> ! {
    // Give all the services time to populate the address book
    // and see who is on the network.
    Timer::after_secs(20).await;
    loop {
        // Give all the services time to populate the address book
        // and see who is on the network.
        Timer::after_secs(5).await;
        let Some(my_addr) = my_ipaddr() else {
            log_error!("Can't find my IP address");
            continue;
        };

        let book_guard = address_book.lock().await;
        let mut ledger_guard = ledger.lock().await;

        // If the ledger does not exist and I do not have it then
        // I will make one.
        if ledger_guard.is_none() {
            log_info!("Creating new ledger");
            let ledger = Ledger {
                owner: my_addr,
                jobs: FnvIndexSet::new(),
            };
            let state = LedgerState::new(ledger);
            *ledger_guard = Some(state);
            if let Some(ledger_state) = ledger_guard.as_mut() {
                Message::send_ledger(ledger_state.ledger.clone(), udp).await;
            };
            continue;
        }

        let Some(ledger_state) = ledger_guard.as_mut() else {
            log_info!("I do not have the ledger");
            continue;
        };

        // if the ledger has not updated in 45 seconds and the owner is not
        // in the address book any more then we need to try and renew the
        // ledger.
        if ledger_state.received.elapsed() > Duration::from_secs(45)
            && !book_guard.contains_key(&ledger_state.ledger.owner)
        {
            if let Some(min_addr) = book_guard.keys().min() {
                // I am the lowest ip address on the network
                // so I will take over.
                if *min_addr == my_addr {
                    ledger_state.ledger.owner = my_addr;
                }
            } else {
                // No one in the address book so take ownership.
                ledger_state.ledger.owner = my_addr;
            };
        }

        // I have the ledger - let's see if I can manufacture something.
        if ledger_state.is_owner() && marlin::is_ready() && marlin::is_idle() {
            log_info!("Available for Jobs");

            // Find the first printable job
            let printable_job = ledger_state.ledger.jobs.iter().find_map(|&guid| {
                let path = make_path(&guid, false);
                if fs::open(&path, ReadBytes).is_ok() {
                    Some((guid, path))
                } else {
                    None
                }
            });

            if let Some((guid, path)) = printable_job {
                match marlin::print(&path, true) {
                    Ok(_) => {
                        marlin::set_offline();
                        ledger_state.ledger.jobs.remove(&guid);
                        let msg = format!("Job Accepted: {guid}");
                        Message::send_log(&msg, udp).await;
                    }
                    Err(e) => {
                        log_error!("Print Error: {e}");
                    }
                }
            }
        }

        // If I am the owner then I should try and pass the ledger on.
        if ledger_state.is_owner() {
            if book_guard.is_empty() {
                log_info!("It's only me - keeping ledger - and telling everyone.");
                Message::send_ledger(ledger_state.ledger.clone(), udp).await;
                continue;
            }

            let mut next_highest = u8::MAX;
            let mut next_addr: Ipv4Addr = Ipv4Addr::new(255, 255, 255, 255);
            for addr in book_guard.keys() {
                let diff = addr.octets()[3].saturating_sub(my_addr.octets()[3]);
                if diff > 0 && diff < next_highest {
                    // A closer IP address has been found;
                    next_addr = *addr;
                    next_highest = diff;
                }
            }
            // We know there is one from our previous checks
            if next_highest > 0 && next_highest < u8::MAX {
                ledger_state.ledger.owner = next_addr;
            } else {
                // We know there is at least one in the address book.
                let min_addr = book_guard.keys().min().unwrap();
                ledger_state.ledger.owner = *min_addr;
            }

            Message::send_ledger(ledger_state.ledger.clone(), udp).await;
        }
    }
}
