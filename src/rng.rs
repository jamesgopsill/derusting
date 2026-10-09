use core::ffi::c_void;

/// STM32 HAL status codes, as returned by `HAL_RNG_GenerateRandomNumber`.
#[repr(u32)]
#[derive(Debug, PartialEq)]
#[allow(unused)]
pub enum HalStatus {
    Ok = 0x00,
    Error = 0x01,
    Busy = 0x02,
    Timeout = 0x03,
}

// SAFETY: both declarations must match the firmware's STM32 HAL definitions:
// `hrng` is the RNG handle (only its address is used, so it is declared
// opaque) and `HAL_RNG_GenerateRandomNumber` takes that handle and an output
// pointer, returning a HAL status code.
unsafe extern "C" {
    /// Hardware RNG handle owned by the HAL (used by address only).
    static hrng: c_void;
    /// Writes one 32-bit hardware random word to `random`; returns a
    /// `HalStatus` code.
    fn HAL_RNG_GenerateRandomNumber(hrng_ptr: *const c_void, random: *mut u32) -> u32;
}

// SAFETY: `getrandom` looks this symbol up by its unmangled name as its
// custom backend (`__getrandom_v03_custom`); no other item uses that name.
#[unsafe(no_mangle)]
/// Fills `dest[..len]` with hardware random bytes for the `getrandom` crate.
/// Retries while the RNG reports `Busy` and returns an error on any other
/// non-`Ok` status.
///
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
            x if x == HalStatus::Ok as u32 => {
                let bytes = random_val.to_le_bytes();
                let remaining = buf.len() - i;
                let take = core::cmp::min(remaining, 4);
                buf[i..i + take].copy_from_slice(&bytes[..take]);
                i += take;
            }
            // If the hardware is busy (processing entropy), we can try again
            // or return a retry error. For simplicity, we loop/retry.
            // NOTE: Could continually loop if the hardware got stuck. Should
            // introduce a bounded retry.
            x if x == HalStatus::Busy as u32 => continue,
            _ => {
                // Return a custom error if the RNG hardware has a
                // Clock Error or Seed Error.
                return Err(getrandom::Error::UNEXPECTED);
            }
        }
    }
    Ok(())
}
