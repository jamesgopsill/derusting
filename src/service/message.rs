use core::{
    cell::RefCell,
    net::{Ipv4Addr, SocketAddrV4},
};

use embassy_time::{Instant, Timer};
use heapless::{HistoryBuf, Vec};
use rand::{RngExt, rngs::SmallRng};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::service::Vfs;

use super::{BROADCAST_ADDR, UdpSocket};

const MAX_JOBS: usize = 48;

/// The token-ring ledger of pending jobs, tracking which machine currently
/// owns it.
#[derive(Debug, Clone)]
pub(crate) struct OwnedLedger {
    pub owner: Ipv4Addr,
    jobs: Vec<Uuid, MAX_JOBS>,
    pub updated: Instant,
}

impl OwnedLedger {
    pub fn new(owner: Ipv4Addr) -> Self {
        Self {
            owner,
            jobs: Vec::new(),
            updated: Instant::now(),
        }
    }

    pub fn jobs(&self) -> &[Uuid] {
        &self.jobs
    }

    pub fn contains(&self, id: &Uuid) -> bool {
        self.jobs.contains(id)
    }

    /// `Ok(true)` inserted, `Ok(false)` already present, `Err(id)` full.
    pub fn insert(&mut self, id: Uuid) -> Result<bool, Uuid> {
        if self.contains(&id) {
            return Ok(false);
        }
        self.jobs.push(id).map(|()| true)
    }

    /// Removes `id`, keeping the remaining jobs in FIFO order.
    pub fn remove(&mut self, id: &Uuid) -> bool {
        match self.jobs.iter().position(|j| j == id) {
            Some(i) => {
                self.jobs.remove(i);
                true
            }
            None => false,
        }
    }

    pub async fn pick_one<V>(&self) -> Option<Uuid>
    where
        V: Vfs,
    {
        for guid in self.jobs.iter() {
            let path = heapless::format!(64; "/usb/{guid}.gcode").unwrap();
            if V::exists(&path).await {
                return Some(*guid);
            }
        }
        None
    }

    // Borrow as the wire form, e.g. to pass the token on to `next`.
    pub fn share(&self) -> SharedLedger<'_> {
        SharedLedger {
            owner: self.owner,
            jobs: &self.jobs,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SharedLedger<'a> {
    pub owner: Ipv4Addr,
    #[serde(borrow, with = "uuid_slice")]
    jobs: &'a [Uuid],
}

impl From<SharedLedger<'_>> for OwnedLedger {
    /// Take ownership when the token arrives. Re-inserting drops any
    /// duplicate UUIDs a faulty peer may have sent.
    fn from(shared: SharedLedger<'_>) -> Self {
        let mut owned = OwnedLedger::new(shared.owner);
        for id in shared.jobs {
            let _ = owned.insert(*id); // can't overflow: length checked on decode
        }
        owned
    }
}

#[derive(Debug)]
pub struct OwnedGcode {
    pub guid: Uuid,
    pub chunk_id: u32,
    pub last_chunk: bool,
    pub data: [u8; 768],
}

impl OwnedGcode {
    pub fn share(&self) -> SharedGcode<'_> {
        SharedGcode {
            guid: self.guid,
            chunk_id: self.chunk_id,
            last_chunk: self.last_chunk,
            data: &self.data,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SharedGcode<'a> {
    pub guid: Uuid,
    pub chunk_id: u32,
    pub last_chunk: bool,
    #[serde(borrow)]
    pub data: &'a [u8],
}

/// The kinds of message broadcast over UDP between machines.
#[derive(Debug, Serialize, Deserialize)]
pub enum Payload<'a> {
    Heartbeat(&'a str),
    NewJob(Uuid),
    Ledger(SharedLedger<'a>),
    #[serde(borrow)]
    Log(&'a str),
    Gcode(SharedGcode<'a>),
}

/// The envelope every UDP message is wrapped in, carrying an idempotency
/// key used to drop duplicate deliveries.
#[derive(Debug, Serialize, Deserialize)]
pub struct Message<'a> {
    pub idempotency: Uuid,
    #[serde(borrow)]
    pub payload: Payload<'a>,
}

pub struct MessageManager<U>
where
    U: UdpSocket,
{
    udp: U,
    history: RefCell<HistoryBuf<Uuid, 16>>,
}

impl<U> MessageManager<U>
where
    U: UdpSocket,
{
    pub fn new(udp: U) -> Self {
        Self {
            udp,
            history: RefCell::new(HistoryBuf::new()),
        }
    }

    pub async fn receive<'a>(&self, buf: &'a mut [u8]) -> Option<(SocketAddrV4, Message<'a>)> {
        // TODO. better logging
        let mut history = self.history.borrow_mut();
        let Ok((remote, packet)) = self.udp.receive(buf).await else {
            return None;
        };

        let Ok(msg) = postcard::from_bytes::<Message>(packet) else {
            return None;
        };

        if history.contains(&msg.idempotency) {
            return None;
        }

        history.write(msg.idempotency);
        Some((remote, msg))
    }

    pub async fn send_heartbeat(&self) -> Result<(), U::Error> {
        let idempotency = Uuid::new_v4();
        let msg = Message {
            idempotency,
            payload: Payload::Heartbeat("HB"),
        };
        if let Ok(packet) = postcard::to_slice(&msg, &mut [0u8; 1024]) {
            self.udp.send(BROADCAST_ADDR, packet).await?;
        }
        Ok(())
    }

    pub async fn send_share(&self, share: SharedLedger<'_>) -> Result<(), U::Error> {
        let idempotency = Uuid::new_v4();
        let msg = Message {
            idempotency,
            payload: Payload::Ledger(share),
        };
        if let Ok(packet) = postcard::to_slice(&msg, &mut [0u8; 1024]) {
            self.udp.send(BROADCAST_ADDR, packet).await?;
        }
        Ok(())
    }

    pub async fn send_gcode(&self, gcode: SharedGcode<'_>) -> Result<(), U::Error> {
        let idempotency = Uuid::new_v4();
        let msg = Message {
            idempotency,
            payload: Payload::Gcode(gcode),
        };
        if let Ok(packet) = postcard::to_slice(&msg, &mut [0u8; 1024]) {
            self.udp.send(BROADCAST_ADDR, packet).await?;
            let mut rng: SmallRng = rand::make_rng();
            let jitter: u64 = rng.random_range(50..=150);
            Timer::after_millis(jitter).await;
            self.udp.send(BROADCAST_ADDR, packet).await?;
        }
        Ok(())
    }

    pub async fn send_new_job(&self, guid: Uuid) -> Result<(), U::Error> {
        let idempotency = Uuid::new_v4();
        let msg = Message {
            idempotency,
            payload: Payload::NewJob(guid),
        };
        if let Ok(packet) = postcard::to_slice(&msg, &mut [0u8; 1024]) {
            self.udp.send(BROADCAST_ADDR, packet).await?;
        }
        Ok(())
    }

    pub async fn send_log(&self, fargs: core::fmt::Arguments<'_>) -> Result<(), U::Error> {
        // TODO. fix this
        let msg = heapless::format!(128; "{}", fargs).unwrap();
        let idempotency = Uuid::new_v4();
        let msg = Message {
            idempotency,
            payload: Payload::Log(&msg),
        };
        if let Ok(packet) = postcard::to_slice(&msg, &mut [0u8; 512]) {
            self.udp.send(BROADCAST_ADDR, packet).await?;
        }
        Ok(())
    }
}

/// Wire format for a job list: one length-prefixed byte string of `n * 16`
/// bytes, borrowed straight out of the packet buffer on receive.
mod uuid_slice {
    use super::MAX_JOBS;
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _};
    use uuid::Uuid;

    pub fn serialize<S: Serializer>(jobs: &[Uuid], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(bytemuck::cast_slice(jobs))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<&'de [Uuid], D::Error> {
        let bytes: &'de [u8] = Deserialize::deserialize(d)?;
        let jobs: &'de [Uuid] = bytemuck::try_cast_slice(bytes)
            .map_err(|_| D::Error::invalid_length(bytes.len(), &"a multiple of 16 bytes"))?;
        if jobs.len() > MAX_JOBS {
            return Err(D::Error::invalid_length(jobs.len(), &"at most 32 jobs"));
        }
        Ok(jobs)
    }
}
