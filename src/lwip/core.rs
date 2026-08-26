use crate::lwip::bindings::{lock_tcpip_core, sys_mutex_lock, sys_mutex_unlock};

#[derive(Debug)]
pub struct LwipCore;

pub fn with_lwip_core(fcn: impl FnOnce(LwipCore)) {
    unsafe { sys_mutex_lock(&raw mut lock_tcpip_core) };
    fcn(LwipCore);
    unsafe { sys_mutex_unlock(&raw mut lock_tcpip_core) };
}
