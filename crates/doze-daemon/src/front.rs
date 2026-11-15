//! The daemon's front: for each instance doze sits in front of, a TCP listener on its declared address. A connection
//! wakes the instance (and so what it needs), is counted as a holder while it lasts, and is spliced to wherever the
//! engine says the instance is reached (a private port, or a Unix socket).
//!
//! This is the plain-TCP core of D2's listeners: no names, no TLS termination, no `wire.accept` hand-off yet.

use doze_engine::{Cause, Engine};

/// A line in the daemon's log (its stderr; no tracing subscriber is installed in `doze`).
macro_rules! say {
    ($($t:tt)*) => {{
        #[allow(clippy::print_stderr)]
        {
            eprintln!("doze daemon: {}", format_args!($($t)*));
        }
    }};
}
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// The listeners, open; dropping them does not close them, [`Front::close`] does. Instances added at run time get
/// theirs with [`Front::add`]; a removed one's closes with [`Front::remove`].
#[derive(Debug)]
pub struct Front {
    engine: Engine,
    tasks: std::sync::Mutex<std::collections::BTreeMap<String, JoinHandle<()>>>,
}

/// One instance doze fronts: its name and the address clients use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fronted {
    pub name: String,
    pub addr: String,
}

impl Front {
    /// Binds every address; one that cannot be bound is said in the log and the rest still open (Go refused to
    /// start; the app shows the instance failing to wake instead, which names the port's holder).
    pub async fn open(engine: &Engine, fronted: &[Fronted]) -> Front {
        let front = Front {
            engine: engine.clone(),
            tasks: std::sync::Mutex::default(),
        };
        for f in fronted {
            if let Err(err) = front.add(f).await {
                say!("{err}");
            }
        }
        front
    }

    /// Binds one more address (an instance added at run time); the error names the instance and the address.
    pub async fn add(&self, f: &Fronted) -> Result<(), String> {
        let l = TcpListener::bind(&f.addr)
            .await
            .map_err(|err| format!("{} cannot listen on {}: {err}", f.name, f.addr))?;
        let task = tokio::spawn(accept(self.engine.clone(), f.name.clone(), l));
        if let Some(old) = crate::lock(&self.tasks).insert(f.name.clone(), task) {
            old.abort();
        }
        Ok(())
    }

    /// Closes one instance's listener (it was removed); its open connections end on their own.
    pub fn remove(&self, name: &str) {
        if let Some(t) = crate::lock(&self.tasks).remove(name) {
            t.abort();
        }
    }

    /// Closes every listener (open connections end with the runtime).
    pub fn close(&self) {
        for t in crate::lock(&self.tasks).values() {
            t.abort();
        }
    }
}

async fn accept(engine: Engine, name: String, l: TcpListener) {
    loop {
        match l.accept().await {
            Ok((conn, peer)) => {
                tokio::spawn(splice(engine.clone(), name.clone(), conn, peer));
            }
            Err(err) => {
                say!("front {name}: accept: {err}");
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
    }
}

async fn splice(engine: Engine, name: String, mut conn: TcpStream, peer: SocketAddr) {
    let ep = match engine.wake(&name, Cause::Connection).await {
        Ok(ep) => ep,
        Err(err) => {
            say!("front {name}: a connection from {peer} could not wake it: {err}");
            return;
        }
    };
    let dialed = match ep.backend.strip_prefix("unix:") {
        Some(p) => doze_os::net::connect_unix(p).await,
        None => doze_os::net::connect_tcp(&ep.backend).await,
    };
    let mut backend = match dialed {
        Ok(b) => b,
        Err(err) => {
            say!("front {name}: {}: {err}", ep.backend);
            return;
        }
    };
    engine.acquire(&name);
    let _ = tokio::io::copy_bidirectional(&mut conn, &mut backend).await;
    engine.release(&name);
}
