//! The daemon's front (core plan D2): for each instance doze sits in front of, a listener on every port it has, where
//! [`crate::names`] put it (its own address, or 127.0.0.1). A connection wakes the instance (and so what it needs),
//! is counted as a holder while it lasts, and is spliced to wherever the engine says the instance is reached: its
//! main listener to the engine's endpoint, another listener (a console, a schema registry) to the backend its module
//! was granted for it. A module that takes its clients itself (`wire.accept`: the postgres filter, which answers
//! PostgreSQL's first packets and terminates TLS) is handed the connection instead, and wakes it and counts it through
//! the host's callbacks.

use crate::names::{BindError, Zone};
use doze_bind::{Bound, Decl, Plan};
use doze_engine::{Cause, Engine};
use doze_module_host::{BoxFuture, Running};
use doze_module_proto::msg::console::WireAccept;
use doze_module_proto::msg::instance::Address;
use doze_module_proto::network::MAIN;
use doze_names::Lease;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// A line in the daemon's log (its stderr; no tracing subscriber is installed in `doze`).
macro_rules! say {
    ($($t:tt)*) => {{
        #[allow(clippy::print_stderr)]
        {
            eprintln!("doze daemon: {}", format_args!($($t)*));
        }
    }};
}

/// One instance doze fronts, as the plan needs it.
#[derive(Debug, Clone)]
pub struct Fronted {
    pub name: String,
    /// Its listeners and port, for [`doze_bind::plan`].
    pub decl: Decl,
    /// An app or service: with an address of its own it is also on 127.0.0.1 at its port, from habit
    /// (`localhost:3000`), when nothing else has that.
    pub app: bool,
    /// Its own domains (`dev.acme.com`), and the certificate the person brought for them.
    pub domains: Vec<String>,
    pub cert: String,
    pub key: String,
    /// Where the engine listens for its main listener (a module's socket file or a loopback port chosen at each start;
    /// an app's is the engine's to choose).
    pub main_backend: Address,
}

/// A module that takes a connection itself, and what it is told with it.
#[derive(Debug)]
pub struct Wire {
    pub module: Arc<Running>,
    pub accept: WireAccept,
}

/// What the front asks of the daemon about a connection.
pub trait Targets: Send + Sync {
    /// Where `listener` (not the main one) of an awake instance is reached.
    fn listener(&self, instance: &str, listener: &str) -> Option<String>;
    /// The module that takes `instance`'s main-listener connections itself, if it does.
    fn wire<'a>(&'a self, instance: &'a str) -> BoxFuture<'a, Option<Wire>>;
}

/// The listeners, open. Dropping it does not close them; [`Front::close`] does. Instances added at run time get
/// theirs with [`Front::add`]; a removed one's close with [`Front::remove`].
pub struct Front {
    shared: Arc<Shared>,
    zone: Arc<Zone>,
    open: Mutex<BTreeMap<String, Opened>>,
}

impl std::fmt::Debug for Front {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Front")
            .field("zone", &self.zone)
            .finish_non_exhaustive()
    }
}

struct Shared {
    engine: Engine,
    targets: OnceLock<Arc<dyn Targets>>,
}

/// One instance's listeners while they serve.
struct Opened {
    tasks: Vec<JoinHandle<()>>,
    /// Its names and sockets; dropped (released, closed) with it. Domains go first: they are twins of its name.
    domains: Vec<Lease>,
    _bound: Bound,
    /// Where clients reach its main listener (the name's address and port).
    endpoint: String,
    /// An app with an address of its own that could not also have 127.0.0.1 at its port.
    name_only: bool,
}

impl Drop for Opened {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
        for l in self.domains.iter().rev() {
            let _ = l.release();
        }
    }
}

impl Front {
    /// A front with nothing open yet, for `engine`, its names in `zone`.
    pub fn new(engine: &Engine, zone: Arc<Zone>) -> Front {
        Front {
            shared: Arc::new(Shared {
                engine: engine.clone(),
                targets: OnceLock::new(),
            }),
            zone,
            open: Mutex::default(),
        }
    }

    /// Opens every instance `bound` (bound when its stack was assembled), and joins the zone's resolver and front
    /// door.
    pub fn open(
        engine: &Engine,
        zone: Arc<Zone>,
        fronted: &[Fronted],
        mut bound: BTreeMap<String, Bound>,
    ) -> Front {
        let front = Front::new(engine, zone);
        for f in fronted {
            if let Some(b) = bound.remove(&f.name) {
                front.serve(f, b);
            }
        }
        front.zone.serve();
        front
    }

    /// Where the daemon answers what a connection needs (module listeners, hand-offs). Once.
    pub fn set_targets(&self, t: Arc<dyn Targets>) {
        let _ = self.shared.targets.set(t);
    }

    /// The zone the listeners are in.
    pub fn zone(&self) -> &Arc<Zone> {
        &self.zone
    }

    /// Serves an instance whose listeners are bound.
    pub fn serve(&self, f: &Fronted, bound: Bound) {
        let Some(inst) = bound.instances.get(&f.name) else {
            return;
        };
        let mut tasks = Vec::new();
        let mut endpoint = String::new();
        for (l, b) in &inst.listeners {
            if l == MAIN {
                endpoint = b.public.to_string();
            }
            match tokio_listener(&b.socket) {
                Ok(t) => tasks.push(self.accept(&f.name, l, t)),
                Err(e) => say!("front {}: {l}: {e}", f.name),
            }
        }
        let mut name_only = false;
        if f.app
            && self.zone.addressing() == doze_bind::Addressing::Own
            && let Some(main) = inst.listeners.get(MAIN)
        {
            let local = SocketAddr::from((doze_bind::LOOPBACK, main.public.port()));
            match std::net::TcpListener::bind(local).and_then(|l| {
                l.set_nonblocking(true)?;
                TcpListener::from_std(l)
            }) {
                Ok(t) => tasks.push(self.accept(&f.name, MAIN, t)),
                Err(_) => {
                    name_only = true;
                    say!(
                        "{}: {local} is taken, so it is reachable by name only",
                        f.name
                    );
                }
            }
        }
        let domains = self.zone.domains(f, &bound);
        let opened = Opened {
            tasks,
            domains,
            _bound: bound,
            endpoint,
            name_only,
        };
        crate::lock(&self.open).insert(f.name.clone(), opened);
    }

    fn accept(&self, name: &str, listener: &str, l: TcpListener) -> JoinHandle<()> {
        tokio::spawn(accept(
            self.shared.clone(),
            name.to_owned(),
            listener.to_owned(),
            l,
        ))
    }

    /// Binds and serves one more instance (added at run time). A listener just closed by a remove is let go once its
    /// task is dropped: an address in use is tried again for half a second. The error names the instance and the
    /// address.
    pub async fn add(&self, f: &Fronted, run_dir: &Path) -> Result<(), String> {
        let plan: Plan = self.zone.plan(std::slice::from_ref(f))?;
        let mut tries = 0;
        let bound = loop {
            match self.zone.bind(&plan, f, run_dir) {
                Ok(b) => break b,
                Err(BindError::Busy(_)) if tries < 10 => {
                    tries += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                Err(e) => return Err(e.to_string()),
            }
        };
        self.serve(f, bound);
        Ok(())
    }

    /// Closes one instance's listeners and gives its names back (it was removed); its open connections end on their
    /// own.
    pub fn remove(&self, name: &str) {
        let gone = crate::lock(&self.open).remove(name);
        drop(gone);
        self.zone.forget(name);
    }

    /// Closes every listener and gives every name back (open connections end with the runtime). The zone's resolver
    /// and front door are left with [`Zone::close`].
    pub fn close(&self) {
        let all = std::mem::take(&mut *crate::lock(&self.open));
        drop(all);
    }

    /// Where clients reach `name`'s main listener, if doze fronts it.
    pub fn endpoint(&self, name: &str) -> Option<String> {
        crate::lock(&self.open)
            .get(name)
            .map(|o| o.endpoint.clone())
    }

    /// Whether `name` could be given only its name, not also 127.0.0.1.
    pub fn name_only(&self, name: &str) -> bool {
        crate::lock(&self.open)
            .get(name)
            .is_some_and(|o| o.name_only)
    }
}

/// A tokio listener on a duplicate of `s` (the original stays with its lease and forwarding).
fn tokio_listener(s: &std::net::TcpListener) -> std::io::Result<TcpListener> {
    let l = s.try_clone()?;
    l.set_nonblocking(true)?;
    TcpListener::from_std(l)
}

async fn accept(shared: Arc<Shared>, name: String, listener: String, l: TcpListener) {
    loop {
        match l.accept().await {
            Ok((conn, peer)) => {
                tokio::spawn(connection(
                    shared.clone(),
                    name.clone(),
                    listener.clone(),
                    conn,
                    peer,
                ));
            }
            Err(err) => {
                say!("front {name}: accept: {err}");
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
    }
}

async fn connection(
    shared: Arc<Shared>,
    name: String,
    listener: String,
    conn: TcpStream,
    peer: SocketAddr,
) {
    let targets = shared.targets.get().cloned();
    if listener == MAIN
        && let Some(t) = &targets
        && let Some(w) = t.wire(&name).await
    {
        let handed = conn.into_std().and_then(|c| {
            c.set_nonblocking(false)?;
            Ok(c)
        });
        match handed {
            Ok(c) => {
                if let Err(e) = w.module.wire_accept(c, &w.accept).await {
                    say!("front {name}: the hand-off of a connection from {peer} failed: {e}");
                }
            }
            Err(e) => say!("front {name}: {e}"),
        }
        return;
    }
    let engine = &shared.engine;
    let ep = match engine.wake(&name, Cause::Connection).await {
        Ok(ep) => ep,
        Err(err) => {
            say!("front {name}: a connection from {peer} could not wake it: {err}");
            return;
        }
    };
    let backend = if listener == MAIN {
        ep.backend
    } else {
        match targets.and_then(|t| t.listener(&name, &listener)) {
            Some(b) => b,
            None => {
                say!("front {name}: its {listener} listener has no backend");
                return;
            }
        }
    };
    splice(engine, &name, conn, &backend).await;
}

async fn splice(engine: &Engine, name: &str, mut conn: TcpStream, backend: &str) {
    let dialed = match backend.strip_prefix("unix:") {
        Some(p) => doze_os::net::connect_unix(p).await,
        None => doze_os::net::connect_tcp(backend).await,
    };
    let mut upstream = match dialed {
        Ok(b) => b,
        Err(err) => {
            say!("front {name}: {backend}: {err}");
            return;
        }
    };
    // Counted against this declaration of it: a connection that outlives a remove and an add of the same name does
    // not count against the new one.
    let held = engine.hold(name);
    let _ = tokio::io::copy_bidirectional(&mut conn, &mut upstream).await;
    drop(held);
}

/// An address as the front dials it: a socket path with `unix:` before it, or `host:port`.
pub fn dial(a: &Address) -> String {
    match a {
        Address::Unix(p) => format!("unix:{p}"),
        Address::Tcp(t) => t.clone(),
    }
}
