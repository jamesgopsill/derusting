use core::net::Ipv4Addr;

use heapless::index_set::FnvIndexSet;
use serde::{Deserialize, Serialize};
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
pub struct Chunk<'a> {
    pub guid: Uuid,
    pub chunk_id: u16,
    pub last_chunk: bool,
    pub len: u16,
    // Note. limiting to 768 for now
    // as the pack is 1024 in size.
    #[serde(borrow)]
    pub chunk: &'a [u8],
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Ledger {
    pub owner: Ipv4Addr,
    pub jobs: FnvIndexSet<Uuid, 16>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum NetworkMessage<'a> {
    Heartbeat(Heartbeat),
    NewJob(NewJob),
    #[serde(borrow)]
    Chunk(Chunk<'a>),
    Ledger(Ledger),
}

impl<'a> NetworkMessage<'a> {
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
