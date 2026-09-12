use core::net::Ipv4Addr;

use crate::lwip::bindings::netif_default;

pub mod bindings;
pub mod ipaddr;
pub mod packet_buffer;
pub mod tcp;
pub mod udp;

pub fn my_ipaddr() -> Option<Ipv4Addr> {
    if unsafe { netif_default.is_null() } {
        return None;
    }
    let addr = unsafe { Ipv4Addr::from_bits(u32::from_be((*netif_default).ip_addr.addr)) };
    Some(addr)
}
