use embassy_time::Timer;
use postcard::accumulator::{CobsAccumulator, FeedResult};

use crate::{
    log_error, log_info,
    lwip::{packet_buffer::ZeroCopyPacketBuffer, udp::UdpSocket},
    tasks::messages::NetworkMessage,
};

#[embassy_executor::task(pool_size = 1)]
pub async fn heartbeat(sock: &'static UdpSocket) {
    loop {
        if let Some(mut pbuf) = ZeroCopyPacketBuffer::alloc() {
            let hb = NetworkMessage::heartbeat();
            let res = pbuf.with_payload(|payload| postcard::to_slice(&hb, payload).is_ok());
            if res && sock.broadcast(pbuf, 9000).is_err() {
                log_error!("Broadcast failed.");
            }
        }
        Timer::after_millis(500).await;
    }
}

#[embassy_executor::task(pool_size = 1)]
pub async fn udp_receiver(sock: &'static UdpSocket) {
    loop {
        let (_addr, msg) = sock.packets.receive().await;

        let mut accumulator: CobsAccumulator<1024> = CobsAccumulator::new();

        for chunk in msg.iter() {
            // feed() returns the remaining unused bytes from the chunk
            match accumulator.feed::<NetworkMessage>(chunk) {
                FeedResult::Consumed => break, // Need more data
                FeedResult::Success {
                    data: _,
                    remaining: _,
                } => {
                    log_info!("udp_recevier(): Packet Deserialised")
                }
                FeedResult::DeserError(_rem) => {
                    log_error!("udp_receiver(): Deserialization error")
                }
                FeedResult::OverFull(_rem) => {
                    log_error!("udp_receiver(): Overfull error")
                }
            };
        }
    }
}
