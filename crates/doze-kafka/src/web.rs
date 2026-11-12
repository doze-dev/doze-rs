//! The web console beside the broker: bound before anything is created (not the data directory, not its lock file),
//! served until the broker is told to stop.
//!
//! The default address is `127.0.0.1:9093`, the Kafka port plus one, and a default that cannot be had (a second broker
//! on the machine has it) leaves the broker running without a console and says so: an optional page does not stop a
//! broker. An address the operator gave that cannot be bound stops the start with exit 2, as every start that cannot
//! be honoured does. `--console=false` leaves the broker exactly as it was before there was a console.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use kafka_broker::Broker;
use kafka_server::{Config as ServerConfig, Running as Kafka};
use kafka_storage::fs::RealFs;
use tokio::net::TcpListener;

use crate::cli::{Console, DEFAULT_CONSOLE_ADDR};

/// A console that is bound and not yet serving.
#[derive(Debug)]
pub(crate) struct Bound {
    listener: TcpListener,
}

/// A console that is serving.
#[derive(Debug)]
pub(crate) struct Serving(kafka_console::Running);

/// Where the console listens, from `--console-addr`: a host name is resolved, an empty host is every interface.
async fn resolve(addr: &str) -> Result<SocketAddr> {
    let text = if addr.starts_with(':') {
        format!("0.0.0.0{addr}")
    } else {
        addr.to_owned()
    };
    tokio::net::lookup_host(&text)
        .await
        .with_context(|| format!("console address {addr:?}"))?
        .next()
        .with_context(|| format!("console address {addr:?} names no address"))
}

/// Binds the console's listener, if there is to be one. `Err` is a start that must stop (exit 2): an address the operator
/// chose that cannot be had.
pub(crate) async fn bind(console: &Console) -> Result<Option<Bound>> {
    bind_with_default(console, DEFAULT_CONSOLE_ADDR).await
}

/// [`bind`] with the default address given, so a test can make it busy.
async fn bind_with_default(console: &Console, default: &str) -> Result<Option<Bound>> {
    if !console.enabled {
        tracing::info!("the console is off (--console=false)");
        return Ok(None);
    }
    let explicit = console.addr.as_deref();
    let wanted = explicit.unwrap_or(default);
    let bound = async {
        let addr = resolve(wanted).await?;
        TcpListener::bind(addr)
            .await
            .with_context(|| format!("binding the console to {addr}"))
    }
    .await;
    match (bound, explicit) {
        (Ok(listener), _) => Ok(Some(Bound { listener })),
        (Err(e), Some(_)) => Err(e.context("config: the console address cannot be used")),
        (Err(e), None) => {
            tracing::warn!(
                "the console is off: {e:#} (give --console-addr another address, or --console=false to stop saying this)"
            );
            Ok(None)
        }
    }
}

/// What the console is told of how the broker was started.
fn facts(
    server: &ServerConfig,
    kafka: &Kafka,
    listener: &kafka_broker::Listener,
    console: &SocketAddr,
    allow: &[String],
) -> kafka_console::Facts {
    kafka_console::Facts {
        listen: kafka.local_addr(),
        advertised_host: listener.advertised_host.clone(),
        advertised_port: u16::try_from(listener.advertised_port).unwrap_or(0),
        #[cfg(feature = "tls")]
        tls: server.tls.is_some(),
        #[cfg(not(feature = "tls"))]
        tls: false,
        sasl: server
            .sasl
            .as_ref()
            .map(|a| a.offered().iter().map(|m| m.name().to_owned()).collect())
            .unwrap_or_default(),
        kafka_major: server.profile.major().get(),
        allow_hosts: allow.to_vec(),
        unprotected: !console.ip().is_loopback(),
    }
}

impl Bound {
    /// Starts serving: from here the console reads and acts on `broker` as `User:__console`.
    pub(crate) fn serve(
        self,
        console: &Console,
        server: &ServerConfig,
        kafka: &Kafka,
        broker: &Arc<Broker<RealFs>>,
        recorder: Option<Arc<kafka_console::Recorder>>,
    ) -> Result<Serving> {
        let addr = self.listener.local_addr()?;
        let listener = server.advertise(kafka.local_addr());
        let facts = facts(server, kafka, &listener, &addr, &console.allow_hosts);
        let backend = kafka_console::Backend::new(Arc::clone(broker), listener, server.profile);
        let mut live = kafka_console::Live::new(backend, facts);
        if let Some(r) = recorder {
            live = live.with_recorder(r);
        }
        let source = kafka_console::Source::Live(Box::new(live));
        let settings = kafka_console::Settings {
            addr,
            prefix: console.prefix.clone(),
            allow_hosts: console.allow_hosts.clone(),
        };
        let running = kafka_console::start_on(self.listener, source, settings)?;
        tracing::info!(addr = %running.local_addr(), prefix = %console.prefix, "the console is listening");
        Ok(Serving(running))
    }
}

impl Serving {
    /// Stops the console: every open stream ends.
    pub(crate) async fn shutdown(self) {
        self.0.shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn console(enabled: bool, addr: Option<&str>) -> Console {
        Console {
            enabled,
            addr: addr.map(str::to_owned),
            ..Console::default()
        }
    }

    #[tokio::test]
    async fn a_default_address_that_is_taken_leaves_the_broker_with_no_console() {
        let taken = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let busy = taken.local_addr().unwrap().to_string();
        assert!(
            bind_with_default(&console(true, None), &busy)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn an_address_the_operator_gave_that_is_taken_is_an_error_that_says_so() {
        let taken = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let busy = taken.local_addr().unwrap().to_string();
        let e = bind_with_default(&console(true, Some(&busy)), "127.0.0.1:0")
            .await
            .unwrap_err();
        assert!(
            format!("{e:#}").contains("the console address cannot be used"),
            "{e:#}"
        );
    }

    #[tokio::test]
    async fn a_console_that_is_off_binds_nothing_and_a_free_address_is_bound() {
        let off = console(false, Some("127.0.0.1:1"));
        assert!(
            bind_with_default(&off, "127.0.0.1:1")
                .await
                .unwrap()
                .is_none()
        );
        let bound = bind_with_default(&console(true, Some("127.0.0.1:0")), "x")
            .await
            .unwrap()
            .unwrap();
        assert!(bound.listener.local_addr().unwrap().ip().is_loopback());
    }

    #[tokio::test]
    async fn an_empty_host_is_every_interface_and_a_name_is_resolved() {
        assert_eq!(resolve(":0").await.unwrap(), "0.0.0.0:0".parse().unwrap());
        assert!(resolve("localhost:9093").await.unwrap().ip().is_loopback());
        assert!(resolve("no-such-host.invalid:1").await.is_err());
    }
}
