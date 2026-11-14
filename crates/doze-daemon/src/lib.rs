//! The doze daemon (Go's `internal/daemon` and `internal/control/server.go`): one stack's [`doze_engine::Engine`]
//! served over control protocol 2 on a local socket, so the CLI and the desktop app drive it as they drove Go's.
//!
//! The wire is doze-control's (NDJSON, the hello, ids, `cancel`, the frames); this crate is the server side: the
//! connection loop ((`session`)), the unary ops ((`unary`)), the streams and topics ((`topics`)), and the instance
//! view the wire carries ([`view`]). What the engine does not know about an instance (its engine name, version, kind,
//! endpoint words) comes from the config as [`Labels`]; the daemon never reads doze.hcl itself.
//!
//! Connections are threads, as in the fixture daemon: a request runs on its own thread, which waits on the engine
//! through the runtime's handle. A unary request runs to its end even if its client goes (Go ran them on the daemon's
//! context); a stream ends with its connection or its `cancel`.

mod session;
mod topics;
mod unary;
pub mod view;

pub use view::Labels;

use doze_engine::Engine;
use doze_os::local::LocalListener;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
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
    pub(crate) labels: BTreeMap<String, Labels>,
    pub(crate) info: StackInfo,
    pub(crate) rt: tokio::runtime::Handle,
    pub(crate) socket: PathBuf,
    pub(crate) stopping: AtomicBool,
    pub(crate) conns: AtomicUsize,
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
            labels,
            info,
            rt: tokio::runtime::Handle::current(),
            socket,
            stopping: AtomicBool::new(false),
            conns: AtomicUsize::new(0),
        };
        Daemon {
            inner: Arc::new(inner),
        }
    }

    /// The engine it serves.
    pub fn engine(&self) -> &Engine {
        &self.inner.engine
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
