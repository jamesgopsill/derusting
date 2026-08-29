use ::core::{ffi::c_void, ptr, sync::atomic::Ordering};

use portable_atomic::AtomicPtr;
use static_cell::StaticCell;

use crate::{
    log_error, log_info,
    lwip::{
        self,
        bindings::lwip_pcb,
        callbacks::on_tcp_accept,
        tcp::OutThreadTcpProtocolControlBlock,
        tcp_socket::TcpSocket,
        udp::{UdpProtocolControlBlock, UdpSocket},
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
pub static UDP_SOCKET: StaticCell<UdpSocket> = StaticCell::new();

// Static handles for our TCP Service.
pub static TCP_SERVICE_PCB: AtomicPtr<lwip_pcb> = AtomicPtr::new(ptr::null_mut());
pub static TCP_SOCKET: StaticCell<TcpSocket> = StaticCell::new();

pub fn init_udp_service() -> Option<&'static UdpSocket> {
    let mut sock: Option<&'static UdpSocket> = None;
    lwip::core::with_lwip_core(|core| {
        if let Ok(pcb) = UdpProtocolControlBlock::new(&core) {
            match pcb.bind(9000, &core) {
                Err(_) => {
                    log_error!("Failed to bind on 9000");
                    pcb.remove(&core);
                }
                Ok(_) => {
                    let s = UDP_SOCKET.init(UdpSocket::new(pcb));
                    s.pcb.lock(|rc| rc.borrow_mut().recv(s, &core));
                    log_info!("UDP Service Available on 9000...");
                    sock = Some(s);
                }
            }
        } else {
            log_error!("UDP block not created")
        }
    });
    sock
}

pub fn init_tcp_service() -> &'static TcpSocket {
    let tcp_socket = TCP_SOCKET.init(TcpSocket::default());
    lwip::core::with_lwip_core(|core| match OutThreadTcpProtocolControlBlock::new(&core) {
        Ok(tcp) => {
            let err = tcp.bind(8080, &core);
            match err {
                Ok(_) => {
                    if let Ok(tcp) = tcp.listen_with_backlog(1, &core) {
                        tcp.arg(tcp_socket as *mut _ as *mut c_void, &core);
                        tcp.accept(Some(on_tcp_accept), &core);
                        TCP_SERVICE_PCB.store(tcp.as_mut_ptr(), Ordering::SeqCst);
                        log_info!("TCP UP on 8080...");
                    }
                }
                Err(_) => {
                    let _ = tcp.close(&core);
                }
            }
        }
        Err(_) => {
            log_error!("TCP block not created");
        }
    });
    tcp_socket
}
