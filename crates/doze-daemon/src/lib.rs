//! The doze daemon (Go's `internal/daemon` and `internal/control/server.go`): one stack's [`doze_engine::Engine`]
//! served over control protocol 2 on a local socket, so the CLI and the desktop app drive it as they drove Go's.
//!
//! The wire is doze-control's (NDJSON, the hello, ids, `cancel`, the frames); this crate is the server side: the
//! connection loop ((`session`)), the unary ops ((`unary`)), the streams and topics ((`topics`)), the instance
//! view the wire carries ([`view`]), `doze __daemon` itself ([`entry`]) and its lifecycle from outside ([`ctl`]). What the engine does not know about an instance (its engine name, version, kind,
//! endpoint words) comes from the config as [`Labels`]; the daemon never reads doze.hcl itself.
//!
//! Connections are threads, as in the fixture daemon: a request runs on its own thread, which waits on the engine
//! through the runtime's handle. A unary request runs to its end even if its client goes (Go ran them on the daemon's
//! context); a stream ends with its connection or its `cancel`.

mod admin;
mod blocks;
pub mod catalog;
mod client;
mod console;
pub mod ctl;
pub mod downloads;
pub mod entry;
pub mod front;
pub mod lint;
pub mod live;
mod meters;
pub mod names;
pub mod planners;
mod session;
pub mod spec_tree;
pub mod tls;
mod topics;
mod unary;
pub mod view;
mod wire;

pub use view::Labels;

use doze_engine::Engine;
use doze_os::local::LocalListener;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::Duration;

/// Where a stack's daemon listens (Go's `ControlSocketPath`): `<data_dir>/run/doze.admin.sock`.
pub fn socket_path(data_dir: &Path) -> PathBuf {
    data_dir.join("run").join("doze.admin.sock")
}

/// What the status op says about the stack as a whole (Go's `Response` fields of `Status`).
#[derive(Debug, Clone, Default)]
pub struct StackInfo {
    /// The proxy's address (`listen`), empty without one.
    pub listen: String,
    /// The folder doze.hcl is in.
    pub dir: String,
    /// When the config was loaded.
    pub loaded: Option<std::time::SystemTime>,
}

/// One stack's daemon: the engine, what the config says about each instance, and the socket.
#[derive(Debug, Clone)]
pub struct Daemon {
    pub(crate) inner: Arc<Inner>,
}

#[derive(Debug)]
pub(crate) struct Inner {
    pub(crate) engine: Engine,
    /// What the config says of each instance (instances added or removed at run time change it).
    pub(crate) labels: RwLock<BTreeMap<String, Labels>>,
    pub(crate) info: StackInfo,
    pub(crate) rt: tokio::runtime::Handle,
    pub(crate) socket: PathBuf,
    pub(crate) stopping: AtomicBool,
    pub(crate) conns: AtomicUsize,
    /// Recent meter readings.
    pub(crate) meters: meters::Ring,
    /// Each module instance, for its data, console and client.
    pub(crate) modules: RwLock<entry::Modules>,
    /// What adds an instance at run time (`None`: this daemon cannot).
    pub(crate) live: Option<live::Live>,
    /// Instances an add, remove or migrate is changing now: one at a time per name.
    pub(crate) busy: Mutex<std::collections::BTreeSet<String>>,
}

/// A running server; dropping it does not stop it, [`Serving::stop`] does.
#[derive(Debug)]
pub struct Serving {
    daemon: Daemon,
    accept: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Daemon {
    /// A daemon for `engine`, listening at [`socket_path`] of its stack's data folder. Call within a tokio runtime.
    pub fn new(engine: Engine, labels: BTreeMap<String, Labels>, info: StackInfo) -> Daemon {
        let socket = socket_path(&engine.stack().data_dir);
        Daemon::at(engine, labels, info, socket)
    }

    /// The same, at another socket path (tests, a temporary home).
    pub fn at(
        engine: Engine,
        labels: BTreeMap<String, Labels>,
        info: StackInfo,
        socket: PathBuf,
    ) -> Daemon {
        let inner = Inner {
            engine,
            labels: RwLock::new(labels),
            info,
            rt: tokio::runtime::Handle::current(),
            socket,
            stopping: AtomicBool::new(false),
            conns: AtomicUsize::new(0),
            meters: meters::Ring::default(),
            modules: RwLock::default(),
            live: None,
            busy: Mutex::default(),
        };
        Daemon {
            inner: Arc::new(inner),
        }
    }

    /// The module instances it serves data and consoles for. Before [`Daemon::serve`] and before it is cloned.
    #[must_use]
    pub fn with_modules(mut self, modules: entry::Modules) -> Daemon {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.modules = RwLock::new(modules);
        }
        self
    }

    /// What adds instances at run time (the `add` and `remove` ops). Before [`Daemon::serve`] and before it is cloned.
    #[must_use]
    pub fn with_live(mut self, live: live::Live) -> Daemon {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.live = Some(live);
        }
        self
    }

    /// Has `front` ask this daemon what a connection needs: a module's other listeners, and a module that takes its
    /// clients itself (handed them with `tls`, the stack's client TLS).
    pub fn answer_front(&self, front: &front::Front, tls: Option<tls::WireTls>) {
        front.set_targets(std::sync::Arc::new(wire::Targets::new(self, tls)));
    }

    /// What the config says of `name`.
    pub(crate) fn label(&self, name: &str) -> Option<Labels> {
        read(&self.inner.labels).get(name).cloned()
    }

    /// Every instance's labels, as they are now.
    pub(crate) fn labels(&self) -> BTreeMap<String, Labels> {
        read(&self.inner.labels).clone()
    }

    /// Module instance `name`, if it is one.
    pub(crate) fn module(&self, name: &str) -> Option<Arc<doze_engine::modules::ModuleInstance>> {
        read(&self.inner.modules).get(name).cloned()
    }

    /// The engine it serves.
    pub fn engine(&self) -> &Engine {
        &self.inner.engine
    }

    /// Keeps recent meter readings while nobody subscribes to them (Go's `runMeters`), until aborted.
    pub fn keep_meters(&self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(meters::keep(self.clone()))
    }

    /// Where it listens.
    pub fn socket(&self) -> &Path {
        &self.inner.socket
    }

    /// Binds the socket (a leftover file at the path is replaced, as Go's `NewServer` did; its folder is made 0700)
    /// and accepts connections on a thread of their own.
    pub fn serve(&self) -> io::Result<Serving> {
        if let Some(dir) = self.inner.socket.parent() {
            doze_os::fs::private_dir(dir)?;
        }
        let _ = std::fs::remove_file(&self.inner.socket);
        let listener = LocalListener::bind(&self.inner.socket)?;
        let d = self.clone();
        let accept = std::thread::Builder::new()
            .name("doze-accept".into())
            .spawn(move || d.accept_loop(&listener))?;
        Ok(Serving {
            daemon: self.clone(),
            accept: Mutex::new(Some(accept)),
        })
    }

    fn accept_loop(&self, listener: &LocalListener) {
        while !self.inner.stopping.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok(stream) if !self.inner.stopping.load(Ordering::SeqCst) => {
                    let d = self.clone();
                    let id = self.inner.conns.fetch_add(1, Ordering::SeqCst);
                    let spawned = std::thread::Builder::new()
                        .name("doze-conn".into())
                        .spawn(move || session::serve(&d, id, stream));
                    if let Err(err) = spawned {
                        tracing::warn!("control: a connection could not be served: {err}");
                    }
                }
                Ok(_) => break,
                Err(err) => {
                    tracing::warn!("control: accept: {err}");
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        }
    }
}

impl Serving {
    /// Stops accepting and removes the socket (Go: on the daemon's context's end). Connections already open end with
    /// their clients; the engine is the caller's to shut down.
    pub fn stop(&self) {
        let inner = &self.daemon.inner;
        inner.stopping.store(true, Ordering::SeqCst);
        // The accept is blocking: one connection of our own wakes it to see the flag.
        let _ = doze_os::local::LocalStream::connect(&inner.socket);
        if let Some(t) = lock(&self.accept).take() {
            let _ = t.join();
        }
        let _ = std::fs::remove_file(&inner.socket);
    }
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn read<T>(m: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    m.read().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn write<T>(m: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    m.write().unwrap_or_else(PoisonError::into_inner)
}

/// An add, remove or migrate holding one instance name; dropping it lets the next one in.
pub(crate) struct Claim<'a> {
    d: &'a Daemon,
    name: String,
}

impl Daemon {
    /// Holds `name` for an add, remove or migrate; `Err` says another one is changing it.
    pub(crate) fn claim(&self, name: &str) -> Result<Claim<'_>, String> {
        if !lock(&self.inner.busy).insert(name.to_owned()) {
            return Err(format!(
                "instance {name:?} is being added, removed or migrated; try again when that ends"
            ));
        }
        Ok(Claim {
            d: self,
            name: name.to_owned(),
        })
    }
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        lock(&self.d.inner.busy).remove(&self.name);
    }
}
