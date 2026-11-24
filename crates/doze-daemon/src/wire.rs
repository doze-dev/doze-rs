//! What the front asks the daemon about a connection (D2): where a module's other listeners are reached, and whether
//! its module takes its clients itself (`wire.accept`), with the TLS it terminates them with.

use crate::front::{Wire, dial};
use crate::tls::WireTls;
use crate::{Daemon, Inner};
use doze_module_host::BoxFuture;
use doze_module_proto::msg::console::WireAccept;
use doze_module_proto::msg::hello::caps;
use std::sync::Weak;

/// The daemon's answers, held weakly: the front outlives nothing it asks about.
pub(crate) struct Targets {
    d: Weak<Inner>,
    tls: Option<WireTls>,
}

impl Targets {
    pub(crate) fn new(d: &Daemon, tls: Option<WireTls>) -> Targets {
        Targets {
            d: std::sync::Arc::downgrade(&d.inner),
            tls,
        }
    }

    fn daemon(&self) -> Option<Daemon> {
        self.d.upgrade().map(|inner| Daemon { inner })
    }
}

impl crate::front::Targets for Targets {
    fn listener(&self, instance: &str, listener: &str) -> Option<String> {
        let mi = self.daemon()?.module(instance)?;
        mi.listener_backend(listener).map(|a| dial(&a))
    }

    fn wire<'a>(&'a self, instance: &'a str) -> BoxFuture<'a, Option<Wire>> {
        Box::pin(async move {
            let d = self.daemon()?;
            let mi = d.module(instance)?;
            let live = d.inner.live.as_ref()?;
            let module = live.catalog().module_of(mi.key()).await?;
            if module.manifest.caps & caps::WIRE == 0 {
                return None;
            }
            let accept = WireAccept {
                instance: instance.to_owned(),
                require_tls: self.tls.as_ref().is_some_and(WireTls::required),
                local: false,
                // As it is now: renewed, rotated or replaced since the stack loaded.
                tls: self.tls.as_ref().map(|t| {
                    let p = t.pair();
                    (p.0.clone().into(), p.1.clone().into())
                }),
            };
            Some(Wire { module, accept })
        })
    }
}
