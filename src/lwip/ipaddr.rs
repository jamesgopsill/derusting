use super::bindings::*;

#[allow(unused)]
pub struct IpAddr {
    inner: *const ip_addr_t,
}

#[allow(unused)]
impl IpAddr {
    pub fn addr(&self) -> u32 {
        // SAFETY: `self.inner` is non-null (enforced by `TryFrom`), but
        // note this type carries no lifetime tying it to the pointee, so
        // this is only sound if the `*const ip_addr_t` it was built from
        // outlives this `IpAddr` — a requirement this API does not enforce.
        unsafe { (*self.inner).addr }
    }
}

#[allow(unused)]
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Value is null.")]
    Null,
}

impl TryFrom<*const ip_addr_t> for IpAddr {
    type Error = Error;
    fn try_from(value: *const ip_addr_t) -> Result<Self, Self::Error> {
        if value.is_null() {
            Err(Error::Null)
        } else {
            Ok(Self { inner: value })
        }
    }
}
