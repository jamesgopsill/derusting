use core::{ffi::c_void, net::Ipv4Addr};

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};

use crate::lwip::bindings::{
    derusting_holds_tcpip_core_lock, err_t, netif_default, tcpip_callback,
};

pub mod bindings;
pub mod ipaddr;
pub mod packet_buffer;
pub mod put;
pub mod tcp;
pub mod udp;

pub fn my_ipaddr() -> Option<Ipv4Addr> {
    // SAFETY: `netif_default` is a lwIP-owned global raw pointer; reading it
    // (not dereferencing) is safe, and we check it for null before ever
    // dereferencing below.
    if unsafe { netif_default.is_null() } {
        return None;
    }
    // SAFETY: just checked non-null above; `netif_default` is only ever
    // mutated by lwIP's own (core-locked) code, and this read of a plain
    // `u32` field is not aliasing any Rust-side mutable reference.
    let addr = unsafe { Ipv4Addr::from_bits(u32::from_be((*netif_default).ip_addr.addr)) };
    Some(addr)
}

struct ThreadContext<F>
where
    F: FnMut() -> Result<(), err_t>,
{
    closure: F,
    signal: Signal<CriticalSectionRawMutex, Result<(), err_t>>,
}

// TODO: Handle the scenario that the async call get cancelled (e.g., with_timeout)
// and how to handle the dangling pointer in the C callback. Not using it in this
// way at the moment so should not encounter it.
//
// SAFETY (whole function): `ctx` is a local (stack) variable whose address
// is handed to lwIP via `tcpip_callback` and dereferenced back from the
// callback below. This is only sound because `ctx` — and therefore this
// `async fn`'s stack frame / `Future` — is guaranteed to still be alive and
// un-moved when the callback runs, i.e. this future is polled to completion
// (never dropped early). See the TODO above: if this future is ever
// cancelled before lwIP invokes the callback, `ctx_ptr` becomes dangling
// and the eventual callback is UB.
pub(super) async fn async_lwip<F>(closure: F) -> Result<(), err_t>
where
    F: FnMut() -> Result<(), err_t>,
{
    // callback fcn
    unsafe extern "C" fn _tcpip_async_callback<F>(ctx: *mut c_void)
    where
        F: FnMut() -> Result<(), err_t>,
    {
        // SAFETY: `ctx` was produced from `&mut ThreadContext<F>` below and
        // (per the function-level Safety note) is still valid and
        // exclusively-owned-for-this-call at the point lwIP invokes this
        // callback.
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
    // SAFETY: `_tcpip_async_callback::<F>` matches the `ctx` type it will be
    // invoked with, and `ctx_ptr` is valid per the function-level Safety
    // note above.
    let err = unsafe { tcpip_callback(_tcpip_async_callback::<F>, ctx_ptr) };
    if err != err_t::Ok {
        return Err(err);
    }
    ctx.signal.wait().await
}

pub fn blocking_lwip<F, R>(mut closure: F) -> Result<R, err_t>
where
    F: FnMut() -> Result<R, err_t>,
{
    // SAFETY: `derusting_holds_tcpip_core_lock` takes no arguments. When it
    // reports we don't already hold the lock, we take `lock_tcpip_core` via
    // the same lwIP-provided mutex primitives lwIP itself uses, run the
    // closure while holding it, and always release it afterwards (no early
    // return between lock/unlock), matching lwIP's `LOCK_TCPIP_CORE`/
    // `UNLOCK_TCPIP_CORE` contract for calling its APIs from this thread.
    unsafe {
        if derusting_holds_tcpip_core_lock() {
            closure()
        } else {
            bindings::sys_mutex_lock(&raw mut bindings::lock_tcpip_core);
            let res = closure();
            bindings::sys_mutex_unlock(&raw mut bindings::lock_tcpip_core);
            res
        }
    }
}
