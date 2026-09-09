use core::net::Ipv4Addr;

use heapless::index_set::FnvIndexSet;
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct Heartbeat {
    pub alive: bool,
    pub has_ledger: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NewJob {
    pub guid: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Chunk {
    pub guid: Uuid,
    pub chunk_id: u16,
    pub last_chunk: bool,
    pub len: u16,
    #[serde(with = "BigArray")]
    pub chunk: [u8; 768],
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Ledger {
    pub owner: Ipv4Addr,
    pub jobs: FnvIndexSet<Uuid, 16>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum NetworkMessage {
    Heartbeat(Heartbeat),
    NewJob(NewJob),
    Chunk(Chunk),
    Ledger(Ledger),
}

impl NetworkMessage {
    pub fn heartbeat(has_ledger: bool) -> Self {
        Self::Heartbeat(Heartbeat {
            alive: true,
            has_ledger,
        })
    }

    pub fn new_job(guid: Uuid) -> Self {
        Self::NewJob(NewJob { guid })
    }
}
