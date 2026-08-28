use ::core::{ffi::c_void, ptr, sync::atomic::Ordering};

use portable_atomic::AtomicPtr;
use static_cell::StaticCell;

use crate::{
    log_error, log_info,
    lwip::{
        self,
        bindings::lwip_pcb,
        callbacks::on_tcp_accept,
        tcp::TcpProtocolControlBlock,
        tcp_socket::TcpSocket,
        udp::{UdpChannel, UdpProtocolControlBlock},
    },
};

pub mod bindings;
pub mod callbacks;
pub mod core;
pub mod ipaddr;
pub mod packet_buffer;
pub mod tcp;
pub mod tcp_socket;
pub mod udp;

// Static handles for our UDP Service.
pub static UDP_SERVICE: AtomicPtr<bindings::lwip_pcb> = AtomicPtr::new(ptr::null_mut());
pub static UDP_CHANNEL: StaticCell<UdpChannel> = StaticCell::new();

// Static handles for our TCP Service.
pub static TCP_SERVICE_PCB: AtomicPtr<lwip_pcb> = AtomicPtr::new(ptr::null_mut());
pub static TCP_SOCKET: StaticCell<TcpSocket> = StaticCell::new();

pub fn init_udp_service() -> Option<&'static UdpChannel> {
    let mut udp_channel: Option<&'static UdpChannel> = None;
    lwip::core::with_lwip_core(|core| {
        // UDP Service
        if let Ok(service) = UdpProtocolControlBlock::try_from(&UDP_SERVICE) {
            log_info!("Removing existing UDP service");
            service.remove(&core);
        }
        if let Ok(pcb) = UdpProtocolControlBlock::new(&core) {
            match pcb.bind(9000, &core) {
                Err(_) => {
                    log_error!("Failed to bind on 9000");
                    pcb.remove(&core);
                }
                Ok(_) => {
                    let channel = UDP_CHANNEL.init(UdpChannel::default());
                    pcb.recv(channel, &core);
                    log_info!("UDP Service Available on 9000...");
                    UDP_SERVICE.store(pcb.as_mut_ptr(), Ordering::SeqCst);
                    udp_channel = Some(channel);
                }
            }
        } else {
            log_error!("UDP block not created")
        }
    });
    udp_channel
}

pub fn init_tcp_service() -> &'static TcpSocket {
    let tcp_socket = TCP_SOCKET.init(TcpSocket::default());
    lwip::core::with_lwip_core(|core| {
        // TCP Service
        if let Ok(tcp) = TcpProtocolControlBlock::try_from(&TCP_SERVICE_PCB) {
            log_info!("Removing existing TCP service");
            let _ = tcp.close_with_core(&core);
        }

        match TcpProtocolControlBlock::new(&core) {
            Ok(tcp) => {
                let err = tcp.bind_with_core(8080, &core);
                match err {
                    Ok(_) => {
                        if let Ok(tcp) = tcp.listen_with_backlog_with_core(1, &core) {
                            tcp.arg_with_core(tcp_socket as *mut _ as *mut c_void, &core);
                            tcp.accept_with_core(Some(on_tcp_accept), &core);
                            TCP_SERVICE_PCB.store(tcp.as_mut_ptr(), Ordering::SeqCst);
                            log_info!("TCP UP on 8080...");
                        }
                    }
                    Err(_) => {
                        let _ = tcp.close_with_core(&core);
                    }
                }
            }
            Err(_) => {
                log_error!("TCP block not created");
            }
        }
    });
    tcp_socket
}
