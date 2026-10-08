#![allow(unused)]
use core::{
    alloc,
    net::{Ipv4Addr, SocketAddrV4},
};

use bytemuck::pod_align_to;
use embassy_time::Instant;
use heapless::Vec;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::service::Vfs;

use super::{BROADCAST_ADDR, UdpSocket};

const MAX_JOBS: usize = 32;

/// The token-ring ledger of pending jobs, tracking which machine currently
/// owns it.
#[derive(Debug, Clone)]
pub(crate) struct OwnedLedger {
    pub owner: SocketAddrV4,
    jobs: Vec<Uuid, MAX_JOBS>,
    pub updated: Instant,
}

impl OwnedLedger {
    pub fn new(owner: SocketAddrV4) -> Self {
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

    pub fn pop_front(&mut self) -> Option<Uuid> {
        (!self.jobs.is_empty()).then(|| self.jobs.remove(0))
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
    pub owner: SocketAddrV4,
    #[serde(borrow, with = "uuid_slice")]
    jobs: &'a [Uuid],
}

impl SharedLedger<'_> {
    pub fn jobs(&self) -> &[Uuid] {
        self.jobs
    }

    pub fn contains(&self, id: &Uuid) -> bool {
        self.jobs.contains(id)
    }
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

impl<'a> Message<'a> {
    pub async fn send_heartbeat<U: UdpSocket>(udp: &U) -> Result<(), U::Error> {
        let idempotency = Uuid::new_v4();
        let msg = Self {
            idempotency,
            payload: Payload::Heartbeat("HB"),
        };
        msg.send(udp).await
    }

    pub async fn send_share<U: UdpSocket>(
        share: SharedLedger<'a>,
        udp: &U,
    ) -> Result<(), U::Error> {
        let idempotency = Uuid::new_v4();
        let msg = Self {
            idempotency,
            payload: Payload::Ledger(share),
        };
        Self::send(&msg, udp).await
    }

    pub async fn send_gcode<U: UdpSocket>(gcode: SharedGcode<'a>, udp: &U) -> Result<(), U::Error> {
        let idempotency = Uuid::new_v4();
        let msg = Self {
            idempotency,
            payload: Payload::Gcode(gcode),
        };
        Self::send(&msg, udp).await
    }

    pub async fn send_new_job<U: UdpSocket>(guid: Uuid, udp: &U) -> Result<(), U::Error> {
        let idempotency = Uuid::new_v4();
        let msg = Self {
            idempotency,
            payload: Payload::NewJob(guid),
        };
        Self::send(&msg, udp).await
    }

    pub async fn send_log<U: UdpSocket>(msg: &'a str, udp: &U) -> Result<(), U::Error> {
        let idempotency = Uuid::new_v4();
        let msg = Self {
            idempotency,
            payload: Payload::Log(msg),
        };
        Self::send(&msg, udp).await
    }

    async fn send<U: UdpSocket>(&self, udp: &U) -> Result<(), U::Error> {
        if let Ok(packet) = postcard::to_slice(self, &mut [0u8; 1024]) {
            udp.send(BROADCAST_ADDR, packet).await?;
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
