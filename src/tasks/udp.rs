use core::net::Ipv4Addr;

use alloc::string::ToString as _;
use embassy_time::{Instant, Timer};
use embedded_io::Write as _;
use heapless::CString;
use heapless::index_set::FnvIndexSet;
use uuid::Uuid;

use crate::fs::{self, File, ReadBytes, WriteBytes};
use crate::kinds::{AddressBook, JobLedger};
use crate::lwip::my_ipaddr;
use crate::lwip::packet_buffer::PacketBuffer;
use crate::lwip::udp::UdpSocket;
use crate::tasks::messages::{Chunk, Ledger, NetworkMessage};
use crate::{ADDRESS_BOOK_ENTRIES, UDP_CHANNEL_SIZE, UDP_PORT, marlin};
use crate::{log_error, log_info};

/// This task broadcasts a heartbeat to the network to inform
/// other machines that this machine is alive and on the network.
#[embassy_executor::task(pool_size = 1)]
pub async fn heartbeat(udp: &'static UdpSocket<UDP_CHANNEL_SIZE>, ledger: &'static JobLedger) {
    loop {
        if let Some(addr) = my_ipaddr() {
            log_info!("[{:?}] heartbeat()", addr);
        } else {
            log_info!("[Unknown] heartbeat()");
        }
        let has_ledger = ledger.lock().await.is_some();
        if let Some(pbuf) = PacketBuffer::alloc(NetworkMessage::heartbeat(has_ledger))
            && udp.broadcast(pbuf, UDP_PORT).await.is_err()
        {
            log_error!("Broadcast failed.");
        }
        Timer::after_secs(5).await;
    }
}

struct FileTransferTask {
    fh: Option<File<WriteBytes>>,
    guid: Uuid,
    current_chunk: u16,
    last_chunk_received: Instant,
    partial_path: CString<64>,
    final_path: CString<64>,
}

impl FileTransferTask {
    fn new() -> Self {
        Self {
            fh: None,
            guid: Uuid::nil(),
            current_chunk: 0,
            last_chunk_received: Instant::from_secs(0),
            partial_path: CString::new(),
            final_path: CString::new(),
        }
    }

    fn reset(&mut self) {
        self.fh = None;
        self.guid = Uuid::nil();
        self.current_chunk = 0;
        self.last_chunk_received = Instant::from_secs(0);
        self.partial_path = CString::new();
        self.final_path = CString::new();
    }

    fn digest(&mut self, chunk: Chunk) {
        if self.fh.is_none() && chunk.chunk_id == 1 {
            self.guid = chunk.guid;
            self.partial_path = make_path(&self.guid, true);
            self.final_path = make_path(&self.guid, false);
            if let Ok(mut fh) = fs::open(&self.partial_path, WriteBytes) {
                // TODO: handle errors
                let _ = fh.write(chunk.chunk);
                self.last_chunk_received = Instant::now();
                self.fh = Some(fh);
                return;
            }
        }

        let Some(fh) = self.fh.as_mut() else { return };

        // If it has been too long between expected chunks.
        if self.last_chunk_received.elapsed().as_secs() > 3 {
            log_error!("Last file transfer timeout - Reset");
            let _ = fs::delete(&self.partial_path);
            self.reset();
            return;
        }

        if chunk.guid != self.guid {
            // This chunk is for another file.
            return;
        }

        // If it is the next chunk
        if chunk.chunk_id == self.current_chunk + 1 {
            // Write the chunk
            let _ = fh.write(&chunk.chunk[..chunk.len as usize]);
            self.current_chunk += 1;
            self.last_chunk_received = Instant::now();
            if chunk.last_chunk {
                // EOF chunk -> flush copy file and then
                // move to gcode path
                log_info!("File received");
                self.reset();
                let _ = fs::rname(&self.partial_path, &self.final_path);
            }
        } else if chunk.chunk_id < self.current_chunk + 1 {
            log_info!("Previous chunk received");
        } else {
            log_error!("We missed a chunk. Oh well lets start again.");
            let _ = fs::delete(&self.partial_path);
            self.reset();
        }
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
    let mut file_transfer = FileTransferTask::new();
    loop {
        let (addr, packet) = udp.receive().await;
        log_info!("Recevied packet from: {addr}");

        let Some(msg) = packet.into_iter().next() else {
            continue;
        };

        let Ok(msg) = postcard::from_bytes::<NetworkMessage>(msg) else {
            log_error!("Packet Deserialization failed");
            continue;
        };

        if let NetworkMessage::Heartbeat(hb) = msg {
            let mut book = address_book.lock().await;
            if let Err(e) = book.insert(addr, (Instant::now(), hb.has_ledger)) {
                log_error!("Address book error: {e:?}");
            };
            continue;
        }

        if let NetworkMessage::NewJob(new_job) = msg {
            let mut ledge = ledger.lock().await;
            if let Some(l) = ledge.as_mut() {
                let _ = l.jobs.insert(new_job.guid);
            }
            continue;
        }

        // We have received a chunk of a gcode file. We can only process
        // one file at a time. We check if we're not already processing
        // a file. Check whether the chunk_id is the next one in the list.
        // TODO: We need to include the uuid of the job as multiple jobs
        // at the same time might interfere with one another.
        // NOTE: Future me, improve to handle multiple files at the same.
        if let NetworkMessage::Chunk(chunk) = msg {
            file_transfer.digest(chunk);
            continue;
        }

        // Check if I am now the owner of the ledger.
        if let NetworkMessage::Ledger(sent_ledger) = msg {
            let mut l = ledger.lock().await;
            if let Some(my_addr) = my_ipaddr()
                && sent_ledger.owner == my_addr
                && l.is_none()
            {
                // Claim the ledger
                *l = Some(sent_ledger);
            }
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
        book.retain(|_k, v| {
            let elapsed = v.0;
            elapsed.as_secs() < 60
        });
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
    Timer::after_secs(30).await;
    loop {
        // Give all the services time to populate the address book
        // and see who is on the network.
        Timer::after_secs(10).await;
        let Some(my_addr) = my_ipaddr() else {
            log_error!("Can't find my IP address");
            continue;
        };

        let book_guard = address_book.lock().await;
        let mut ledger_guard = ledger.lock().await;

        // v.1 being a bool to indicate if there is a ledger
        // in play
        let ledger_exists = book_guard.iter().any(|(_k, v)| v.1);
        let ledger_is_none = ledger_guard.is_none();
        log_info!(
            "exists: {} ledger_is_none: {}",
            ledger_exists,
            ledger_is_none
        );
        // If the ledger does not exist and I do not have it then
        // I will make one.
        if !ledger_exists && ledger_is_none {
            log_info!("Creating new ledger");
            *ledger_guard = Some(Ledger {
                owner: my_addr,
                jobs: FnvIndexSet::new(),
            });
            // Declare it as soon as possible
            if let Some(pbuf) = PacketBuffer::alloc(NetworkMessage::heartbeat(true))
                && udp.broadcast(pbuf, UDP_PORT).await.is_err()
            {
                log_error!("Broadcast failed.");
            }
            continue;
        }

        let Some(mut ledger) = ledger_guard.take() else {
            log_info!("I do not have the ledger");
            continue;
        };

        // I have the ledger lets see if I can do something.
        if marlin::is_ready() && marlin::is_idle() {
            log_info!("Available for Jobs");

            // Find the first printable job
            let printable_job = ledger.jobs.iter().find_map(|&guid| {
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
                        ledger.jobs.remove(&guid);
                    }
                    Err(e) => {
                        log_error!("Print Error: {e}");
                    }
                }
            }
        }

        if book_guard.is_empty() {
            log_info!("It's only me - keeping ledger");
            *ledger_guard = Some(ledger);
            continue;
        }

        // Pass on the ledger
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
        if next_highest > 0 {
            ledger.owner = next_addr;
        } else {
            // We know there is at lease one in the address book.
            let min_addr = book_guard.keys().min().unwrap();
            ledger.owner = *min_addr;
        }

        // Send the message out to pass it around
        let msg = NetworkMessage::Ledger(ledger);
        for _i in 0..2 {
            if let Some(pbuf) = PacketBuffer::alloc(&msg)
                && udp.broadcast(pbuf, UDP_PORT).await.is_err()
            {
                log_error!("Broadcast failed.");
            }
            Timer::after_millis(200).await;
        }
    }
}

// TODO: file cleanup.

/// Create the full file path for a given uuid.
pub fn make_path(job_guid: &Uuid, partial: bool) -> CString<64> {
    let mut path = CString::<64>::new();
    let _ = path.extend_from_bytes(b"/usb/");
    let _ = path.extend_from_bytes(job_guid.to_string().as_bytes());
    if partial {
        let _ = path.extend_from_bytes(b".partial");
    } else {
        let _ = path.extend_from_bytes(b".gcode");
    }
    path
}
