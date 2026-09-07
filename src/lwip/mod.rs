use ::core::{ffi::c_void, ptr, sync::atomic::Ordering};
use core::net::Ipv4Addr;

use embassy_sync::channel::Channel;
use portable_atomic::AtomicPtr;
use static_cell::StaticCell;

use crate::{
    log_error, log_info,
    lwip::{
        bindings::{lwip_pcb, netif_default},
        callbacks::on_tcp_accept,
        tcp::{OutThread, TcpHandler, TcpProtocolControlBlock},
        udp::{UdpProtocolControlBlock, UdpSocket},
    },
};

pub mod bindings;
pub mod callbacks;
pub mod ipaddr;
pub mod packet_buffer;
pub mod tcp;
pub mod udp;

// Static handles for our UDP Service.
pub const UDP_PORT: u16 = 9000;
pub static UDP_SOCKET: StaticCell<UdpSocket> = StaticCell::new();

/// Initialise the UDP service.
pub fn init_udp_service() -> Option<&'static UdpSocket> {
    let mut sock: Option<&'static UdpSocket> = None;
    match UdpProtocolControlBlock::<udp::OutThread>::new() {
        Ok(pcb) => {
            let s = UDP_SOCKET.init(UdpSocket::new(pcb));
            let ok = s.pcb.lock(|rc| {
                let mut pcb = rc.borrow_mut();
                pcb.with_core(|pcb| {
                    let ok = pcb.bind(UDP_PORT).is_ok();
                    if ok {
                        pcb.recv(s);
                    }
                    ok
                })
            });
            if ok {
                sock = Some(s);
            } else {
                log_error!("Failed to bind to sock");
            };
        }
        Err(_) => log_error!("UDP failed to create"),
    }
    sock
}

// Static handles for our TCP Service.
pub static TCP_SERVICE_PCB: AtomicPtr<lwip_pcb> = AtomicPtr::new(ptr::null_mut());
pub static TCP_HANDLER: StaticCell<TcpHandler> = StaticCell::new();

/// Initialise the TCP service.
pub fn init_tcp_service() -> &'static TcpHandler {
    let tcp_handler = TCP_HANDLER.init(Channel::new());
    match TcpProtocolControlBlock::<OutThread>::new() {
        Ok(mut pcb) => pcb.with_core(|pcb| {
            let err = pcb.bind(8080);
            match err {
                Ok(_) => {
                    if let Ok(pcb) = pcb.listen_with_backlog(1) {
                        pcb.arg(tcp_handler as *mut _ as *mut c_void);
                        pcb.accept(Some(on_tcp_accept));
                        TCP_SERVICE_PCB.store(pcb.as_mut_ptr(), Ordering::SeqCst);
                        log_info!("TCP UP on 8080...");
                    }
                }
                Err(_) => {
                    let _ = pcb.close();
                }
            }
        }),
        Err(_) => log_error!("TCP block not created"),
    }
    tcp_handler
}

pub fn my_ipaddr() -> Option<Ipv4Addr> {
    if unsafe { netif_default.is_null() } {
        return None;
    }
    let addr = unsafe { Ipv4Addr::from_bits(u32::from_be((*netif_default).ip_addr.addr)) };
    Some(addr)
}
