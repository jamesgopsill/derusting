use core::{ffi::c_void, net::Ipv4Addr};

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};

use crate::lwip::bindings::{LwipError, netif_default, tcpip_callback};

pub mod bindings;
pub mod ipaddr;
pub mod packet_buffer;
pub mod put;
pub mod tcp;
pub mod udp;

pub fn my_ipaddr() -> Option<Ipv4Addr> {
    if unsafe { netif_default.is_null() } {
        return None;
    }
    let addr = unsafe { Ipv4Addr::from_bits(u32::from_be((*netif_default).ip_addr.addr)) };
    Some(addr)
}

struct ThreadContext<F>
where
    F: FnMut() -> Result<(), LwipError>,
{
    closure: F,
    signal: Signal<CriticalSectionRawMutex, Result<(), LwipError>>,
}

// TODO: Handle the scenario that the async call get cancelled (e.g., with_timeout)
// and how to handle the dangling pointer in the C callback. Not using it in this
// way at the moment so should not encounter it.
pub async fn execute_in_tcpip_thread<F>(closure: F) -> Result<(), LwipError>
where
    F: FnMut() -> Result<(), LwipError>,
{
    // callback fcn
    unsafe extern "C" fn _tcpip_async_callback<F>(ctx: *mut c_void)
    where
        F: FnMut() -> Result<(), LwipError>,
    {
        let ctx = unsafe { &mut *(ctx as *mut ThreadContext<F>) };
        let res = (ctx.closure)();
        ctx.signal.signal(res);
    }

    // allocate and enqueue the callback
    let mut ctx = ThreadContext {
        closure,
        signal: Signal::new(),
    };

    let ctx_ptr = &mut ctx as *mut _ as *mut c_void;
    let err = unsafe { tcpip_callback(_tcpip_async_callback::<F>, ctx_ptr) };
    if err != LwipError::Ok {
        return Err(err);
    }
    ctx.signal.wait().await
}

/* tcpip_callback_wait not present in lwip on Prusa Buddy board atm.
pub fn blocking_execute_in_tcpip_thread<F>(closure: F) -> Result<(), LwipError>
where
    F: FnMut() -> Result<(), LwipError>,
{
    struct Context<F> {
        closure: F,
        result: Result<(), LwipError>,
    }

    // callback fcn
    unsafe extern "C" fn _tcpip_blocking_callback<F>(ctx: *mut c_void)
    where
        F: FnMut() -> Result<(), LwipError>,
    {
        let ctx = unsafe { &mut *(ctx as *mut Context<F>) };
        ctx.result = (ctx.closure)()
    }

    let mut ctx = Context {
        closure,
        result: Ok(()),
    };

    let ctx_ptr = &mut ctx as *mut _ as *mut c_void;
    let err = unsafe { tcpip_callback_wait(_tcpip_blocking_callback::<F>, ctx_ptr) };
    if err != LwipError::Ok {
        return Err(err);
    }

    ctx.result
}
*/
