//! The web console (`aws-console`), beside the gateway, in this process, on a listener of its own.
//!
//! On by default (`--console on`): served on loopback at the gateway's port + 1 (`127.0.0.1:4567` beside the default
//! `127.0.0.1:4566`; a gateway on port 0 gives the console a free port too), and on 127.0.0.1 even when the gateway listens
//! wider. A default address that is taken is not a failure: doze-aws says so and runs without the console. `--console <addr>`
//! names the address (one that cannot be had stops the start); `--no-console`, `--console off` (or `false`),
//! `DOZE_AWS_CONSOLE=off` or `console = "off"` in doze-aws.toml turn it off: then nothing is bound, the gateway has no
//! observer, and a request costs what it did.
//!
//! With it, the address is taken before anything else is opened, the gateway tells the console's Live ring about each finished
//! call, and the console serves once the gateway listens.
//!
//! The console has no login, so it is served on this machine only: a `--console` address that is not loopback is refused at
//! start. (Serving it further needs the rules doze-kafka's console has for that door; they are not built here.)

use std::net::SocketAddr;
use std::sync::Arc;

use aws_console::{Facts, Flag, LocalData, Recorder, Running, ServiceFacts, Settings};
use tokio::net::TcpListener;

use crate::config::{Config, Source};
use crate::data::Data;
use crate::instance::Instance;
use crate::services::{ALL, Svc};
use crate::wire::Apis;

/// A console whose address is taken and whose record the gateway can feed; not serving yet.
pub(crate) struct Planned {
    listener: TcpListener,
    recorder: Arc<Recorder>,
}

/// What `--console` asks for.
#[derive(Debug, PartialEq, Eq)]
enum Wanted {
    Off,
    /// The default: loopback at the gateway's port + 1, or none when that port is past the last.
    Beside(Option<SocketAddr>),
    At(SocketAddr),
}

/// The words `--console` takes besides an address.
const OFF: &[&str] = &["off", "false", "no"];
const ON: &[&str] = &["on", "true", "yes", ""];

/// Reads `--console` (`given`) beside the gateway's `--listen` address.
fn wanted(given: &str, listen: &str) -> Result<Wanted, String> {
    let word = given.trim().to_ascii_lowercase();
    if OFF.contains(&word.as_str()) {
        return Ok(Wanted::Off);
    }
    if !ON.contains(&word.as_str()) {
        return address(given).map(Wanted::At);
    }
    let gateway: SocketAddr = listen
        .parse()
        .map_err(|e| format!("--listen: `{listen}` is not an address ({e})"))?;
    let ip = if gateway.is_ipv6() && gateway.ip().is_loopback() {
        std::net::Ipv6Addr::LOCALHOST.into()
    } else {
        std::net::Ipv4Addr::LOCALHOST.into()
    };
    let port = match gateway.port() {
        0 => Some(0),
        p => p.checked_add(1),
    };
    Ok(Wanted::Beside(port.map(|p| SocketAddr::new(ip, p))))
}

/// Takes the console's address: the one `--console` names, or by default the one beside the gateway.
///
/// # Errors
/// What to tell the person: the address is not one, is not on this machine alone, or cannot be bound.
pub(crate) async fn plan(cfg: &Config) -> Result<Option<Planned>, String> {
    let addr = match wanted(&cfg.text("console"), &cfg.text("listen"))? {
        Wanted::Off => return Ok(None),
        Wanted::At(addr) => addr,
        Wanted::Beside(Some(addr)) => match TcpListener::bind(addr).await {
            Ok(listener) => return Ok(Some(Planned::on(listener))),
            Err(e) => {
                // Not asked for by name: a port that is taken leaves doze-aws without its console, not without its start.
                tracing::warn!(
                    "doze-aws runs without its console: {addr} cannot be had ({e}). Name another with --console 127.0.0.1:<port>, or turn it off with --no-console."
                );
                return Ok(None);
            }
        },
        Wanted::Beside(None) => {
            tracing::warn!(
                "doze-aws runs without its console: the gateway's port is the last, so there is none after it. Name one with --console 127.0.0.1:<port>."
            );
            return Ok(None);
        }
    };
    let listener = TcpListener::bind(addr).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            format!(
                "doze-aws cannot serve its console on {addr}: the address is in use. Give another with --console (port 0 picks a free one), or leave the flag out to run without the console."
            )
        } else {
            format!("doze-aws cannot serve its console on {addr}: {e}")
        }
    })?;
    Ok(Some(Planned::on(listener)))
}

/// The address `--console` names, which must be this machine's loopback.
fn address(given: &str) -> Result<SocketAddr, String> {
    let addr: SocketAddr = given.parse().map_err(|e| {
        format!(
            "--console: `{given}` is not an address ({e}); give one as 127.0.0.1:4567, or on or off"
        )
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
    fn on(listener: TcpListener) -> Self {
        Self {
            listener,
            recorder: Arc::new(Recorder::new()),
        }
    }

    /// The Live record, which the gateway tells about each finished call and the bus about each call between services.
    pub(crate) fn recorder(&self) -> Arc<Recorder> {
        self.recorder.clone()
    }

    /// Serves the console, now that the gateway listens (`me` has its endpoint), with the services' bus for its Resources space.
    ///
    /// # Errors
    /// The listener could not be served.
    pub(crate) fn start(
        self,
        cfg: &Config,
        me: &Instance,
        (on, peers, data): (&[&'static Svc], Arc<aws_peers::Peers>, Arc<Data>),
        apis: Apis,
    ) -> Result<Running, String> {
        let settings = Settings {
            allow_hosts: cfg.list("console-allow-host"),
        };
        let mut facts = facts(cfg, me, on, &apis);
        facts.peers = Some(peers);
        facts.flags = flags(cfg);
        facts.data = Some(Arc::new(ConsoleData(data)));
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

/// Every setting of the run, as `doze-aws config print` lists them; the secret key's value is left out.
fn flags(cfg: &Config) -> Vec<Flag> {
    let file = cfg.file.as_deref().map(|p| p.display().to_string());
    cfg.all()
        .iter()
        .map(|s| Flag {
            name: s.key.name.to_owned(),
            value: (s.key.name != "secret-key").then(|| s.value.join(",")),
            source: crate::config::describe(s.key, &s.source, file.as_deref()),
            default: s.source == Source::Default,
        })
        .collect()
}

/// The data engine, as the console's Settings reads and resets it: the same engine `/_doze/data` serves `doze-aws data`.
struct ConsoleData(Arc<Data>);

impl std::fmt::Debug for ConsoleData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ConsoleData")
    }
}

impl LocalData for ConsoleData {
    fn report(&self) -> aws_wire::service::BoxFuture<'_, Result<serde_json::Value, String>> {
        Box::pin(async move {
            let parts = self.0.report(None).await.map_err(|r| r.message)?;
            Ok(crate::data::to_json(self.0.dir(), self.0.total(), &parts))
        })
    }

    fn reset(
        &self,
        dry_run: bool,
    ) -> aws_wire::service::BoxFuture<'_, Result<serde_json::Value, String>> {
        Box::pin(async move {
            let done = self.0.reset(None, dry_run).await.map_err(|r| r.message)?;
            Ok(crate::data::reset_json(&done))
        })
    }
}

/// What the console says of this doze-aws: the instance, and every service of the catalogue with its APIs.
fn facts(cfg: &Config, me: &Instance, on: &[&'static Svc], apis: &Apis) -> Facts {
    Facts {
        version: me.version.clone(),
        account: me.account.clone(),
        region: me.region.clone(),
        endpoint: me.public_endpoint().to_owned(),
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
        peers: None,
        flags: Vec::new(),
        data: None,
        lab: aws_console::LabFacts::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Wanted, address, wanted};

    #[test]
    fn by_default_the_console_is_beside_the_gateway_on_loopback() {
        let at = |s: &str| Wanted::Beside(Some(s.parse().unwrap()));
        assert_eq!(wanted("on", "127.0.0.1:4566"), Ok(at("127.0.0.1:4567")));
        assert_eq!(wanted("", "127.0.0.1:4566"), Ok(at("127.0.0.1:4567")));
        assert_eq!(wanted("TRUE", "127.0.0.1:0"), Ok(at("127.0.0.1:0")));
        // A gateway served wider still has its console on this machine alone.
        assert_eq!(wanted("on", "0.0.0.0:4566"), Ok(at("127.0.0.1:4567")));
        assert_eq!(wanted("on", "[::]:4566"), Ok(at("127.0.0.1:4567")));
        assert_eq!(wanted("on", "[::1]:4566"), Ok(at("[::1]:4567")));
        assert_eq!(wanted("on", "127.0.0.1:65535"), Ok(Wanted::Beside(None)));
        for off in ["off", "false", "No"] {
            assert_eq!(wanted(off, "127.0.0.1:4566"), Ok(Wanted::Off), "{off}");
        }
        assert_eq!(
            wanted("127.0.0.1:9000", "127.0.0.1:4566"),
            Ok(Wanted::At("127.0.0.1:9000".parse().unwrap()))
        );
        assert!(wanted("0.0.0.0:9000", "127.0.0.1:4566").is_err());
        assert!(wanted("maybe", "127.0.0.1:4566").is_err());
    }

    #[test]
    fn the_console_is_served_on_loopback_only() {
        assert_eq!(address("127.0.0.1:4567").map(|a| a.port()), Ok(4567));
        assert_eq!(address("[::1]:0").map(|a| a.port()), Ok(0));
        for wide in ["0.0.0.0:4567", "192.168.1.20:4567", "[::]:4567"] {
            let e = address(wide).unwrap_err();
            assert!(e.contains("not a loopback address"), "{e}");
            assert!(e.contains("127.0.0.1:4567"), "{e}");
        }
        for wrong in ["4567", "localhost:4567", ""] {
            let e = address(wrong).unwrap_err();
            assert!(e.contains("is not an address"), "{wrong}: {e}");
        }
    }
}
