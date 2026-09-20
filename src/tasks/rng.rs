use core::ffi::c_void;

use embassy_time::Instant;
use uuid::{Timestamp, Uuid};

// STM32 HAL Status codes
#[repr(u32)]
#[derive(Debug, PartialEq)]
#[allow(unused)]
pub enum HalStatus {
    Ok = 0x00,
    Error = 0x01,
    Busy = 0x02,
    Timeout = 0x03,
}

unsafe extern "C" {
    // Hardware RNG
    static hrng: c_void;
    // The hardware rng fcn.
    fn HAL_RNG_GenerateRandomNumber(hrng_ptr: *const c_void, random: *mut u32) -> HalStatus;
}

#[unsafe(no_mangle)]
/// # Safety
/// Called by the `getrandom` crate as its custom backend; `dest` must be
/// valid for writes of `len` bytes for the duration of this call (the
/// crate upholds this for every call site it generates).
unsafe extern "Rust" fn __getrandom_v03_custom(
    dest: *mut u8,
    len: usize,
) -> Result<(), getrandom::Error> {
    // SAFETY: valid per this function's Safety contract above.
    let buf = unsafe { core::slice::from_raw_parts_mut(dest, len) };

    let mut i = 0;
    while i < buf.len() {
        let mut random_val: u32 = 0;

        // 2. Call the STM32 HAL
        // Note: Using &hrng to get the address of the pointer/struct
        //
        // SAFETY: `hrng` is a `'static` HAL-owned handle and `&mut
        // random_val` is a valid, writable local we own for the call.
        // This assumes the HAL only ever returns one of `HalStatus`'s
        // defined discriminants (0-3); a `#[repr(u32)]` enum received
        // directly as an `extern "C"` return value is undefined behaviour
        // if the C side ever produces any other value.
        let status =
            unsafe { HAL_RNG_GenerateRandomNumber(&hrng as *const c_void, &mut random_val) };

        match status {
            HalStatus::Ok => {
                let bytes = random_val.to_le_bytes();
                let remaining = buf.len() - i;
                let take = core::cmp::min(remaining, 4);
                buf[i..i + take].copy_from_slice(&bytes[..take]);
                i += take;
            }
            // If the hardware is busy (processing entropy), we can try again
            // or return a retry error. For simplicity, we loop/retry.
            HalStatus::Busy => continue,
            _ => {
                // Return a custom error if the RNG hardware has a
                // Clock Error or Seed Error.
                return Err(getrandom::Error::UNEXPECTED);
            }
        }
    }
    Ok(())
}

pub fn generate_uuid_v7() -> Uuid {
    // Get time since boot (ms)
    let now_ms = Instant::now().as_millis();

    // Split into seconds and nanoseconds for the UUID v7 timestamp
    let seconds = now_ms / 1000;
    let nanos = (now_ms % 1000) as u32 * 1_000_000;

    let ts = Timestamp::from_unix(uuid::NoContext, seconds, nanos);

    // This call now automatically uses the STM32 Hardware RNG
    // for the random bits!
    Uuid::new_v7(ts)
}
