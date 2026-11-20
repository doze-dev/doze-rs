//! `--console <addr>`: the web console (`aws-console`), beside the gateway, in this process, on a listener of its own.
//!
//! Off unless the flag is given: nothing is bound, the gateway has no observer, and a request costs what it did. With it, the
//! address is taken before anything else is opened (one that cannot be had stops the start), the gateway tells the console's
//! Live ring about each finished call, and the console serves once the gateway listens.
//!
//! The console has no login, so for now it is served on this machine only: a `--console` address that is not loopback is refused
//! at start. (Serving it further needs the rules doze-kafka's console has for that door; they are not built here yet.)

use std::net::SocketAddr;
use std::sync::Arc;

use aws_console::{Facts, Recorder, Running, ServiceFacts, Settings};
use tokio::net::TcpListener;

use crate::config::Config;
use crate::instance::Instance;
use crate::services::{ALL, Svc};
use crate::wire::Apis;

/// A console whose address is taken and whose record the gateway can feed; not serving yet.
pub(crate) struct Planned {
    listener: TcpListener,
    recorder: Arc<Recorder>,
}

/// Takes the console's address, when `--console` gives one.
///
/// # Errors
/// What to tell the person: the address is not one, is not on this machine alone, or cannot be bound.
pub(crate) async fn plan(cfg: &Config) -> Result<Option<Planned>, String> {
    let given = cfg.text("console");
    if given.is_empty() {
        return Ok(None);
    }
    let addr = address(&given)?;
    let listener = TcpListener::bind(addr).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            format!(
                "doze-aws cannot serve its console on {addr}: the address is in use. Give another with --console (port 0 picks a free one), or leave the flag out to run without the console."
            )
        } else {
            format!("doze-aws cannot serve its console on {addr}: {e}")
        }
    })?;
    Ok(Some(Planned {
        listener,
        recorder: Arc::new(Recorder::new()),
    }))
}

/// The address `--console` names, which must be this machine's loopback.
fn address(given: &str) -> Result<SocketAddr, String> {
    let addr: SocketAddr = given.parse().map_err(|e| {
        format!("--console: `{given}` is not an address ({e}); give one as 127.0.0.1:4567")
    })?;
    if !addr.ip().is_loopback() {
        return Err(format!(
            "--console: {addr} is not a loopback address. The console has no login, so it is served on this machine only: give 127.0.0.1:{port} (or [::1]:{port}).",
            port = addr.port()
        ));
    }
    Ok(addr)
}

impl Planned {
    /// The Live record, which the gateway tells about each finished call and the bus about each call between services.
    pub(crate) fn recorder(&self) -> Arc<Recorder> {
        self.recorder.clone()
    }

    /// Serves the console, now that the gateway listens (`me` has its endpoint).
    ///
    /// # Errors
    /// The listener could not be served.
    pub(crate) fn start(
        self,
        cfg: &Config,
        me: &Instance,
        on: &[&'static Svc],
        apis: Apis,
    ) -> Result<Running, String> {
        let settings = Settings {
            allow_hosts: cfg.list("console-allow-host"),
        };
        let facts = facts(cfg, me, on, &apis);
        let running = aws_console::start_on(self.listener, facts, self.recorder, settings)
            .map_err(|e| format!("doze-aws cannot serve its console: {e}"))?;
        // The line a supervisor (and the tests) wait for, beside the gateway's.
        tracing::info!("doze-aws console on {}", running.url());
        Ok(running)
    }
}

/// Stops the console, if there is one: its streams end and its requests in hand are finished.
pub(crate) async fn stop(console: Option<Running>) {
    if let Some(console) = console {
        console.shutdown().await;
    }
}

/// What the console says of this doze-aws: the instance, and every service of the catalogue with its APIs.
fn facts(cfg: &Config, me: &Instance, on: &[&'static Svc], apis: &Apis) -> Facts {
    Facts {
        version: me.version.clone(),
        account: me.account.clone(),
        region: me.region.clone(),
        endpoint: me.endpoint.clone(),
        auth: me.auth.clone(),
        iam: cfg.text("iam"),
        data_dir: me.data_dir.clone(),
        pid: me.pid,
        started: me.started,
        services: ALL
            .iter()
            .map(|svc| ServiceFacts {
                name: svc.name,
                title: svc.title,
                served: on.iter().any(|s| s.name == svc.name),
                apis: apis
                    .iter()
                    .filter(|(of, _)| of.name == svc.name)
                    .map(|(_, api)| Arc::clone(api))
                    .collect(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::address;

    #[test]
    fn the_console_is_served_on_loopback_only() {
        assert_eq!(address("127.0.0.1:4567").map(|a| a.port()), Ok(4567));
        assert_eq!(address("[::1]:0").map(|a| a.port()), Ok(0));
        for wide in ["0.0.0.0:4567", "192.168.1.20:4567", "[::]:4567"] {
            let e = address(wide).unwrap_err();
            assert!(e.contains("not a loopback address"), "{e}");
            assert!(e.contains("127.0.0.1:4567"), "{e}");
        }
        for wrong in ["true", "4567", "localhost:4567", ""] {
            let e = address(wrong).unwrap_err();
            assert!(e.contains("is not an address"), "{wrong}: {e}");
        }
    }
}
