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

/// Broadcast periodically to announce this machine is alive on the network.
#[derive(Debug, Serialize, Deserialize)]
pub struct Heartbeat {
    pub alive: bool,
}

/// Broadcast to announce a newly uploaded job to the rest of the network.
#[derive(Debug, Serialize, Deserialize)]
pub struct NewJob {
    pub guid: Uuid,
}

/// The token-ring ledger of pending jobs, tracking which machine currently
/// owns it.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Ledger {
    pub owner: Ipv4Addr,
    pub jobs: FnvIndexSet<Uuid, 8>,
}

/// The envelope every UDP message is wrapped in, carrying an idempotency
/// key used to drop duplicate deliveries.
#[derive(Debug, Serialize, Deserialize)]
pub struct Message<'a> {
    pub idempotency: Uuid,
    #[serde(borrow)]
    pub payload: Payload<'a>,
}

/// The kinds of message broadcast over UDP between machines.
#[derive(Debug, Serialize, Deserialize)]
pub enum Payload<'a> {
    Heartbeat(Heartbeat),
    NewJob(NewJob),
    Ledger(Ledger),
    #[serde(borrow)]
    Log(&'a str),
}

impl<'a> Message<'a> {
    /// Broadcasts a `Heartbeat` message once.
    pub async fn send_heartbeat<const N: usize>(udp: &UdpSocket<N>) {
        let msg = Self {
            idempotency: generate_uuid_v7(),
            payload: Payload::Heartbeat(Heartbeat { alive: true }),
        };
        Self::send(udp, msg, 1).await;
    }

    /// Broadcasts a `NewJob` message, repeated a few times to tolerate
    /// packet loss.
    pub async fn send_new_job_alert<const N: usize>(guid: Uuid, udp: &UdpSocket<N>) {
        let msg = Self {
            idempotency: generate_uuid_v7(),
            payload: Payload::NewJob(NewJob { guid }),
        };
        Self::send(udp, msg, 5).await;
    }

    /// Broadcasts the current `Ledger`, repeated a few times to tolerate
    /// packet loss.
    pub async fn send_ledger<const N: usize>(ledger: Ledger, udp: &UdpSocket<N>) {
        let msg = Self {
            idempotency: generate_uuid_v7(),
            payload: Payload::Ledger(ledger),
        };
        Self::send(udp, msg, 5).await;
    }

    pub async fn send_log<const N: usize>(log: &'a str, udp: &UdpSocket<N>) {
        let msg = Self {
            idempotency: generate_uuid_v7(),
            payload: Payload::Log(log),
        };
        Self::send(udp, msg, 2).await;
    }

    /// Serialises and broadcasts `msg` over UDP, `repeats` times with a
    /// short delay between each, to tolerate packet loss.
    async fn send<const N: usize>(udp: &UdpSocket<N>, msg: Self, repeats: usize) {
        for _ in 0..repeats {
            {
                let Some(pbuf) = PacketBuffer::alloc(&msg) else {
                    return;
                };
                let _ = udp.broadcast(pbuf, UDP_PORT).await;
            }
            Timer::after_millis(200).await
        }
    }
}
