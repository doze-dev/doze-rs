//! The daemon's front: for each instance doze sits in front of, a TCP listener on its declared address. A connection
//! wakes the instance (and so what it needs), is counted as a holder while it lasts, and is spliced to wherever the
//! engine says the instance is reached (a private port, or a Unix socket).
//!
//! This is the plain-TCP core of D2's listeners: no names, no TLS termination, no `wire.accept` hand-off yet.

use doze_engine::{Cause, Engine};
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// The listeners, open; dropping them does not close them, [`Front::close`] does.
#[derive(Debug, Default)]
pub struct Front {
    tasks: Vec<JoinHandle<()>>,
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
    #[allow(clippy::print_stderr)] // the daemon's stderr is its log file
    pub async fn open(engine: &Engine, fronted: &[Fronted]) -> Front {
        let mut tasks = Vec::new();
        for f in fronted {
            match TcpListener::bind(&f.addr).await {
                Ok(l) => {
                    let (e, name) = (engine.clone(), f.name.clone());
                    tasks.push(tokio::spawn(accept(e, name, l)));
                }
                Err(err) => eprintln!("doze daemon: {} cannot listen on {}: {err}", f.name, f.addr),
            }
        }
        Front { tasks }
    }

    /// Closes every listener (open connections end with the runtime).
    pub fn close(&self) {
        for t in &self.tasks {
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
                tracing::warn!("front {name}: accept: {err}");
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
    }
}

async fn splice(engine: Engine, name: String, mut conn: TcpStream, peer: SocketAddr) {
    let ep = match engine.wake(&name, Cause::Connection).await {
        Ok(ep) => ep,
        Err(err) => {
            tracing::warn!("front {name}: a connection from {peer} could not wake it: {err}");
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
            tracing::warn!("front {name}: {}: {err}", ep.backend);
            return;
        }
    };
    engine.acquire(&name);
    let _ = tokio::io::copy_bidirectional(&mut conn, &mut backend).await;
    engine.release(&name);
}
