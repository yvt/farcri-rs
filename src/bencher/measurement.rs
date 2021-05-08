use cryo::{CryoMutWriteGuard, LocalLock};

use super::{protocol, proxylink};

pub(super) struct Measurement<'link> {
    // `cryo` is used here to hide `Criterion`'s lifetime. We do this to
    // simplify the interface and to keep it close to that of Criterion.rs.
    link: CryoMutWriteGuard<proxylink::ProxyLink<'link>, LocalLock>,
}

pub type Instant = protocol::Instant;
pub type Duration = protocol::Duration;

impl<'link> Measurement<'link> {
    #[inline]
    pub fn new(link: CryoMutWriteGuard<proxylink::ProxyLink<'link>, LocalLock>) -> Self {
        Self { link }
    }

    #[inline]
    pub fn link(&mut self) -> &mut proxylink::ProxyLink<'link> {
        &mut self.link
    }

    #[inline]
    pub fn value(&mut self) -> u64 {
        self.link.io().now()
    }

    pub fn now(&mut self) -> Instant {
        self.link.send(&protocol::UpstreamMessage::GetInstant);

        match self.link.recv() {
            protocol::DownstreamMessage::Instant(x) => x,
            other => {
                panic!("unexpected downstream message: {:?}", other);
            }
        }
    }
}
