use super::bindings::*;

#[allow(unused)]
pub struct IpAddr {
    inner: *const lwip_ipaddr,
}

#[allow(unused)]
impl IpAddr {
    pub fn addr(&self) -> u32 {
        unsafe { (*self.inner).addr }
    }
}

impl TryFrom<*const lwip_ipaddr> for IpAddr {
    type Error = ();
    fn try_from(value: *const lwip_ipaddr) -> Result<Self, Self::Error> {
        if value.is_null() {
            Err(())
        } else {
            Ok(Self { inner: value })
        }
    }
}
