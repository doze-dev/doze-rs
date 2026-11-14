//! The schema registry beside the broker: bound before anything is created (as the console is), served from the broker's
//! metadata database once it is open, stopped before the broker.
//!
//! A default address that cannot be had (a second broker on the machine has 8081) leaves the broker running without a
//! registry and says so; an address the operator gave that cannot be bound stops the start with exit 2. Records the
//! registry cannot read leave the broker running without it, and nothing in the database is changed (as in the Go edition).

use std::sync::Arc;

use anyhow::{Context, Result};
use kafka_broker::Broker;
use kafka_registry::api::{Api, Auth, Options};
use kafka_registry::registry::Registry as Store;
use kafka_registry::server::Running;
use kafka_registry::store::MetaStorage;
use kafka_server::Config as ServerConfig;
use kafka_storage::fs::RealFs;
use tokio::net::TcpListener;

use crate::cli::Registry;

/// A registry that is bound and not yet serving.
#[derive(Debug)]
pub(crate) struct Bound {
    listener: TcpListener,
    registry: Registry,
}

/// Binds the registry's listener, if there is to be one.
///
/// # Errors
/// An address the operator chose that cannot be bound.
/// Why there is no registry.
#[derive(Debug, Clone)]
pub(crate) enum Off {
    Disabled,
    Busy(String),
    Unreadable(String),
}

#[cfg(feature = "console")]
impl Off {
    fn console(&self) -> kafka_console::schemas::Off {
        match self {
            Self::Disabled => kafka_console::schemas::Off::Disabled,
            Self::Busy(why) => kafka_console::schemas::Off::Busy(why.clone()),
            Self::Unreadable(why) => kafka_console::schemas::Off::Unreadable(why.clone()),
        }
    }
}

/// The registry as it started: serving (the store, for the console in this process, and its address), or why not.
#[derive(Debug)]
pub(crate) enum Status {
    On {
        store: Arc<Store>,
        url: String,
        basic_auth: bool,
    },
    Off(Off),
}

#[cfg(feature = "console")]
impl Status {
    /// What the console is given.
    pub(crate) fn console(&self) -> kafka_console::schemas::Schemas {
        match self {
            Self::On {
                store,
                url,
                basic_auth,
            } => kafka_console::schemas::Schemas::On {
                registry: Arc::clone(store),
                url: url.clone(),
                basic_auth: *basic_auth,
            },
            Self::Off(why) => kafka_console::schemas::Schemas::Off(why.console()),
        }
    }
}

pub(crate) async fn bind(r: &Registry) -> Result<Result<Bound, Off>> {
    if !r.enabled {
        tracing::info!("the schema registry is off (--schema-registry=false)");
        return Ok(Err(Off::Disabled));
    }
    let bound = async {
        let addr = tokio::net::lookup_host(&r.addr)
            .await
            .with_context(|| format!("schema registry address {:?}", r.addr))?
            .next()
            .with_context(|| format!("schema registry address {:?} names no address", r.addr))?;
        TcpListener::bind(addr)
            .await
            .with_context(|| format!("binding the schema registry to {addr}"))
    }
    .await;
    match bound {
        Ok(listener) => Ok(Ok(Bound {
            listener,
            registry: r.clone(),
        })),
        Err(e) if r.chosen => Err(e.context(
            "config: the schema registry address cannot be used (choose another with --schema-registry-addr, or turn it off with --schema-registry=false)",
        )),
        Err(e) => {
            tracing::warn!(
                "the schema registry is off: {e:#} (give --schema-registry-addr another address, or --schema-registry=false to stop saying this)"
            );
            Ok(Err(Off::Busy(format!("{e:#}"))))
        }
    }
}

/// The broker's SASL users as the registry's basic credentials: the users configured at start, checked as a PLAIN login is.
fn auth(server: &ServerConfig) -> Option<Auth> {
    let sasl = server.sasl.clone()?;
    Some(Arc::new(move |user: &str, pass: &str| {
        let token = format!("\0{user}\0{pass}");
        sasl.plain(token.as_bytes()).is_ok()
    }))
}

impl Bound {
    /// Starts serving the registry over the broker's metadata database. `None` when its records cannot be read: the broker
    /// runs on without it.
    pub(crate) fn serve(
        self,
        server: &ServerConfig,
        broker: &Arc<Broker<RealFs>>,
    ) -> Result<(Option<Running>, Status)> {
        let r = &self.registry;
        let store = match Store::open(Box::new(MetaStorage(Arc::clone(broker.meta())))) {
            Ok(store) => store,
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "the schema registry is off: its records could not be read; the broker runs without it, and nothing in meta.redb was changed"
                );
                return Ok((None, Status::Off(Off::Unreadable(e))));
            }
        };
        let store = Arc::new(store);
        let options = Options {
            auth: auth(server),
            allowed_hosts: r.allow_hosts.clone(),
        };
        let basic_auth = options.auth.is_some();
        let api = Arc::new(Api::new(Arc::clone(&store), options));
        #[cfg(feature = "tls")]
        let tls = match (&server.tls, r.tls) {
            (Some(t), true) => kafka_registry::server::tls(t.server_config()),
            _ => None,
        };
        #[cfg(not(feature = "tls"))]
        let tls = ();
        let running = kafka_registry::server::start_on(self.listener, api, tls)
            .context("starting the schema registry")?;
        let scheme = if r.tls { "https" } else { "http" };
        let url = format!("{scheme}://{}", running.local_addr());
        tracing::info!(url = %url, "the schema registry is listening");
        Ok((
            Some(running),
            Status::On {
                store,
                url,
                basic_auth,
            },
        ))
    }
}

/// Starts the registry that `main` bound, if it bound one.
///
/// # Errors
/// The listener has no address.
pub(crate) fn start(
    bound: Result<Bound, Off>,
    server: &ServerConfig,
    broker: &Arc<Broker<RealFs>>,
) -> Result<(Option<Running>, Status)> {
    match bound {
        Ok(b) => b.serve(server, broker),
        Err(off) => Ok((None, Status::Off(off))),
    }
}

/// Stops the registry, if one is serving.
pub(crate) async fn stop(running: Option<Running>) {
    if let Some(r) = running {
        r.shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(addr: &str, chosen: bool) -> Registry {
        Registry {
            enabled: true,
            addr: addr.to_owned(),
            chosen,
            tls: false,
            allow_hosts: Vec::new(),
        }
    }

    #[tokio::test]
    async fn a_busy_default_leaves_no_registry_and_a_busy_chosen_address_stops_the_start() {
        let taken = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let busy = taken.local_addr().unwrap().to_string();
        assert!(matches!(
            bind(&registry(&busy, false)).await.unwrap(),
            Err(Off::Busy(_))
        ));
        let e = bind(&registry(&busy, true)).await.unwrap_err();
        assert!(format!("{e:#}").contains("--schema-registry-addr"), "{e:#}");
        let off = Registry {
            enabled: false,
            ..registry("127.0.0.1:1", true)
        };
        assert!(matches!(bind(&off).await.unwrap(), Err(Off::Disabled)));
        assert!(
            bind(&registry("127.0.0.1:0", true))
                .await
                .unwrap()
                .is_ok()
        );
    }
}
