use embassy_time::Timer;

use crate::{
    log_error, log_info,
    lwip::{packet_buffer::ZeroCopyPacketBuffer, udp::UdpSocket},
    tasks::messages::NetworkMessage,
};

#[embassy_executor::task(pool_size = 1)]
pub async fn heartbeat(sock: &'static UdpSocket) {
    loop {
        if let Some(pbuf) = ZeroCopyPacketBuffer::alloc(NetworkMessage::heartbeat()) {
            if sock.broadcast(pbuf, 9000).is_err() {
                log_error!("Broadcast failed.");
            } else {
                log_info!("Heartbeat broadcasted on 9000");
            }
        }
        Timer::after_secs(5).await;
    }
}

#[embassy_executor::task(pool_size = 1)]
pub async fn udp_receiver(sock: &'static UdpSocket) {
    log_info!("Ready to receive UDP packets");
    loop {
        let (_addr, msg) = sock.packets.receive().await;
        log_info!("Packet received");
        if let Some(msg) = msg.iter().next() {
            match postcard::from_bytes::<NetworkMessage>(msg) {
                Ok(network_msg) => match network_msg {
                    NetworkMessage::Heartbeat(h) => log_info!("Heartbeat: alive={}", h.alive),
                },
                Err(_) => log_error!("Deserialization failed for packet: {:02X?}", msg),
            }
        }
    }
}
