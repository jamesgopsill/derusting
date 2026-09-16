use core::net::Ipv4Addr;

use embassy_time::Timer;
use heapless::index_set::FnvIndexSet;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    UDP_PORT,
    lwip::{packet_buffer::PacketBuffer, udp::UdpSocket},
    tasks::rng::generate_uuid_v7,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Heartbeat {
    pub alive: bool,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Ledger {
    pub owner: Ipv4Addr,
    pub jobs: FnvIndexSet<Uuid, 8>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Message<'a> {
    pub idempotency: Uuid,
    #[serde(borrow)]
    pub payload: Payload<'a>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Payload<'a> {
    Heartbeat(Heartbeat),
    NewJob(NewJob),
    #[serde(borrow)]
    Chunk(Chunk<'a>),
    Ledger(Ledger),
}

impl<'a> Message<'a> {
    pub async fn send_heartbeat<const N: usize>(udp: &UdpSocket<N>) {
        let msg = Self {
            idempotency: generate_uuid_v7(),
            payload: Payload::Heartbeat(Heartbeat { alive: true }),
        };
        Self::send(udp, msg).await;
    }

    pub async fn send_new_job_alert<const N: usize>(guid: Uuid, udp: &UdpSocket<N>) {
        let msg = Self {
            idempotency: generate_uuid_v7(),
            payload: Payload::NewJob(NewJob { guid }),
        };
        Self::send(udp, msg).await;
    }

    pub async fn send_ledger<const N: usize>(ledger: Ledger, udp: &UdpSocket<N>) {
        let msg = Self {
            idempotency: generate_uuid_v7(),
            payload: Payload::Ledger(ledger),
        };
        Self::send(udp, msg).await;
    }

    pub async fn send_chunk<const N: usize>(chunk: Chunk<'a>, udp: &UdpSocket<N>) {
        let msg = Self {
            idempotency: generate_uuid_v7(),
            payload: Payload::Chunk(chunk),
        };
        Self::send(udp, msg).await;
    }

    async fn send<const N: usize>(udp: &UdpSocket<N>, msg: Self) {
        for _ in 0..2 {
            {
                let Some(pbuf) = PacketBuffer::alloc(&msg) else {
                    return;
                };
                let _ = udp.broadcast(pbuf, UDP_PORT).await;
            }
            Timer::after_millis(100).await
        }
    }
}
