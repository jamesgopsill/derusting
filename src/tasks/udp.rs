use core::{cell::RefCell, net::Ipv4Addr};

use alloc::string::ToString;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};
use embassy_time::{Instant, Timer};
use embedded_io::Write as _;
use heapless::{CString, LinearMap, index_set::FnvIndexSet};
use uuid::Uuid;

use crate::{
    fs::{File, ReadBytes, WriteBytes},
    log_error, log_info,
    lwip::{UDP_PORT, my_ipaddr, packet_buffer::PacketBuffer, udp::UdpSocket},
    marlin::{is_idle, is_ready, print, set_offline},
    tasks::messages::{Chunk, Ledger, NetworkMessage},
};

type AddressBook = Mutex<ThreadModeRawMutex, RefCell<LinearMap<Ipv4Addr, Instant, 32>>>;
type StaticLedger = Mutex<ThreadModeRawMutex, RefCell<Option<Ledger>>>;

pub static ADDRESS_BOOK: AddressBook = Mutex::new(RefCell::new(LinearMap::new()));
pub static LEDGER: StaticLedger = Mutex::new(RefCell::new(None));

/// This task broadcasts a heartbeat to the network to inform
/// other machines that this machine is alive and on the network.
#[embassy_executor::task(pool_size = 1)]
pub async fn heartbeat(sock: &'static UdpSocket) {
    loop {
        if let Some(addr) = my_ipaddr() {
            log_info!("[{:?}] heartbeat()", addr);
        } else {
            log_info!("[Unknown] heartbeat()");
        }
        let has_ledger = LEDGER.lock().await.borrow().is_some();
        if let Some(pbuf) = PacketBuffer::alloc(NetworkMessage::heartbeat(has_ledger))
            && sock.broadcast(pbuf, UDP_PORT).is_err()
        {
            log_error!("Broadcast failed.");
        }
        Timer::after_secs(5).await;
    }
}

struct FileTransfer {
    fh: File<WriteBytes>,
    guid: Uuid,
    current_chunk: u16,
    last_chunk_received: Instant,
}

impl FileTransfer {
    fn digest(mut self, chunk: Chunk) -> Option<Self> {
        // If it has been too long between expected chunks.
        if self.last_chunk_received.elapsed().as_secs() > 3 {
            log_error!("Last file transfer timeout - Reset");
            self.fh.close();
            let path = make_path(&self.guid, true);
            let _ = File::<ReadBytes>::delete(&path);
            return None;
        }

        if chunk.guid != self.guid {
            // This chunk is for another file.
            // Give self it back
            return Some(self);
        }

        // If it is the next chunk
        if chunk.chunk_id == self.current_chunk + 1 {
            // Write the chunk
            let _ = self.fh.write(&chunk.chunk[..chunk.len as usize]);
            self.current_chunk += 1;
            self.last_chunk_received = Instant::now();
            if chunk.last_chunk {
                // EOF chunk -> close the file
                log_info!("File received");
                self.fh.close();
                let partial_path = make_path(&self.guid, true);
                let gcode_path = make_path(&self.guid, false);
                File::<ReadBytes>::rename(&partial_path, &gcode_path);
                None
            } else {
                Some(self)
            }
        } else if chunk.chunk_id < self.current_chunk + 1 {
            log_info!("Previous chunk received");
            Some(self)
        } else {
            log_error!("We missed a chunk. Oh well lets start again.");
            self.fh.close();
            let path = make_path(&self.guid, true);
            let _ = File::<ReadBytes>::delete(&path);
            None
        }
    }
}

impl TryFrom<Chunk> for FileTransfer {
    type Error = ();

    fn try_from(value: Chunk) -> Result<Self, Self::Error> {
        let path = make_path(&value.guid, true);
        let Ok(mut fh) = File::open(&path, WriteBytes) else {
            log_error!("File Open Error");
            return Err(());
        };
        let _ = fh.write(&value.chunk[..value.len as usize]);
        let ft = Self {
            guid: value.guid,
            current_chunk: 1,
            fh,
            last_chunk_received: Instant::now(),
        };
        Ok(ft)
    }
}

/// This task will receive and handle messages being sent over UDP
/// on the network.
#[embassy_executor::task(pool_size = 1)]
pub async fn udp_receiver(sock: &'static UdpSocket) {
    log_info!("Ready to receive UDP packets");
    let mut file_transfer: Option<FileTransfer> = None;
    loop {
        let (addr, msg) = sock.packets.receive().await;
        if let Some(msg) = msg.into_iter().next() {
            match postcard::from_bytes::<NetworkMessage>(msg) {
                Ok(network_msg) => match network_msg {
                    // We have received a heartbeat from another machine.
                    // Lets add/update their entry in our address book.
                    NetworkMessage::Heartbeat(h) => {
                        log_info!("{}: Heartbeat: alive={}", addr, h.alive);
                        let lock = ADDRESS_BOOK.lock().await;
                        let mut book = lock.borrow_mut();
                        if let Err(e) = book.insert(addr, Instant::now()) {
                            log_error!("Address book error: {e:?}");
                        };
                    }
                    // We have received a new_job message and the owner of the
                    // ledger should update the ledger to include the job in
                    // the list.
                    NetworkMessage::NewJob(new_job) => {
                        let lock = LEDGER.lock().await;
                        let mut ledger = lock.borrow_mut();
                        if let Some(ledger) = ledger.as_mut() {
                            let _ = ledger.jobs.insert(new_job.guid);
                        }
                    }
                    // We have received a chunk of a gcode file. We can only process
                    // one file at a time. We check if we're not already processing
                    // a file. Check whether the chunk_id is the next one in the list.
                    // TODO: We need to include the uuid of the job as multiple jobs
                    // at the same time might interfere with one another.
                    // NOTE: Future me, improve to handle multiple files at the same.
                    NetworkMessage::Chunk(chunk) => {
                        if let Some(ft) = file_transfer.take() {
                            file_transfer = ft.digest(chunk);
                        } else {
                            // No file_transfer present so lets check if the
                            // chunk_id is 1 and a start of a new file.
                            if chunk.chunk_id == 1 {
                                if let Ok(ft) = FileTransfer::try_from(chunk) {
                                    file_transfer = Some(ft);
                                }
                                continue;
                            }
                        }
                    }
                    // We have received a ledger message which occurs when the
                    // ledger is being exchanged. We check if we're the new
                    // owner of the ledger and take control. Otherwise we ignore.
                    NetworkMessage::Ledger(ledger) => {
                        let guard = LEDGER.lock().await;
                        let mut rc = guard.borrow_mut();
                        if let Some(my_addr) = my_ipaddr()
                            && ledger.owner == my_addr
                            && rc.is_none()
                        {
                            *rc = Some(ledger);
                        }
                    }
                },
                Err(_) => {
                    // log_error!("Deserialization failed for packet: {:02X?}", msg)
                    log_error!("Packet Deserialization failed");
                }
            }
        }
    }
}

/// This task periodically checks the address book and cleans up
/// any address have not heard from in a while.
#[embassy_executor::task(pool_size = 1)]
pub async fn address_book_lifetime_check() {
    loop {
        Timer::after_secs(10).await;
        let lock = ADDRESS_BOOK.lock().await;
        let mut book = lock.borrow_mut();
        book.retain(|_, v| {
            let elapsed = v.elapsed();
            elapsed.as_secs() < 60
        });
    }
}

/// This task manages the token-ring ledger that is passed between
/// machines. We only do something if we are the owner of the ledger.
#[embassy_executor::task(pool_size = 1)]
pub async fn manage_ledger(sock: &'static UdpSocket) -> ! {
    loop {
        // Give all the services time to populate the address book
        // and see who is on the network.
        Timer::after_secs(10).await;
        // TODO: Check that the ledger exists with a machine on
        // the network and if not then spawn one.
        //
        let Some(my_addr) = my_ipaddr() else {
            log_error!("Can't find my IP address");
            continue;
        };
        let book_guard = ADDRESS_BOOK.lock().await;
        let book = book_guard.borrow();
        let ledger_guard = LEDGER.lock().await;
        let mut ledger = ledger_guard.borrow_mut();

        if book.is_empty() && ledger.is_none() {
            log_info!("Creating new ledger");
            let new_ledger = Ledger {
                owner: my_addr,
                jobs: FnvIndexSet::new(),
            };
            *ledger = Some(new_ledger);
        }

        if let Some(mut l) = ledger.take() {
            log_info!("{:?}", l);
            if is_ready() && is_idle() {
                log_info!("Available for Jobs");
                // 1. Am I free to take on a job and is there a job in the
                // list I can take. If so, take it and remove it from the
                // list.
                let mut selected_job: Option<Uuid> = None;
                for job_guid in l.jobs.iter() {
                    let path = make_path(job_guid, true);
                    // Use open to see if we have a copy of the file
                    if File::open(&path, ReadBytes).is_ok() {
                        selected_job = Some(*job_guid);
                    }
                }
                if let Some(job_guid) = selected_job {
                    l.jobs.remove(&job_guid);
                    let path = make_path(&job_guid, true);
                    match print(&path, true) {
                        Ok(_) => set_offline(),
                        Err(e) => {
                            log_error!("Print Error: {e}");
                        }
                    };
                    set_offline();
                } else {
                    log_info!("No jobs found.");
                }
            } else {
                if !is_ready() {
                    log_info!("Not Ready for Printing");
                }
                if !is_idle() {
                    log_info!("Not Idle");
                }
            }
            // 2. Pass the ledger on another printer. The next highest in
            // the address book wrapping around or if there is no one else
            // then hold on to the ledger.
            if !book.is_empty() {
                let mut closest = u8::MAX;
                let mut closest_addr: Ipv4Addr = Ipv4Addr::new(255, 255, 255, 255);
                let mut smallest = u8::MAX;
                let mut smallest_addr: Ipv4Addr = Ipv4Addr::new(255, 255, 255, 255);
                for addr in book.keys() {
                    let diff = addr.octets()[3] - my_addr.octets()[3];
                    if diff > 0 && diff < closest {
                        closest = diff;
                        closest_addr = *addr;
                    }
                    if addr.octets()[3] < smallest {
                        smallest = addr.octets()[3];
                        smallest_addr = *addr;
                    }
                }
                if closest > 0 && closest != u8::MAX {
                    // there is a machine above
                    l.owner = closest_addr;
                } else {
                    // we need to wrap back around
                    l.owner = smallest_addr;
                }
                let msg = NetworkMessage::Ledger(l);
                for _i in 0..3 {
                    // TODO: Consider passing a reference.
                    if let Some(pbuf) = PacketBuffer::alloc(&msg)
                        && sock.broadcast(pbuf, UDP_PORT).is_err()
                    {
                        log_error!("Broadcast failed.");
                    }
                }
            } else {
                log_info!("Book is empty - keeping ledger");
            }
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
