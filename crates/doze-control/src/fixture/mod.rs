//! A daemon that plays a [`Script`] on a real socket, so the app and the CLI are tested through the same protocol path
//! they use against a real daemon (docs/port/app/PLAN.md §4.7). It is Go's `fakedaemon` behind Go's server: the same
//! hello and refusal words, the same unary answers, subscriptions filtered by topic, instance, panel and row, verbs
//! that change the status as a daemon would, and held streams a test can push frames into.
//!
//! One difference from the fakedaemon, taken from the real daemon: a subscription to `instances` is sent a snapshot of
//! the scripted status, because the Go fixture hubs carry no instances frames (Go's dash reads the snapshot
//! separately).

mod play;
mod script;
mod session;
mod unary;

pub use script::{Entry, Fail, Script, ScriptError, Step};

use crate::client::connection::lock;
use crate::{Client, Frame, Request, Response};
use async_channel::Sender;
use doze_os::local::{LocalListener, LocalStream};
use play::Filter;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

/// What a stopped daemon's held streams end with (Go fakedaemon's `ErrGone`).
pub const GONE: &str = "daemon not reachable: connection refused";

/// A message to a running stream.
#[derive(Debug)]
pub(crate) enum Msg {
    Frame(Box<Frame>),
    End(Option<String>),
}

/// A stream being played: where to reach it, what it wants, and whether its script is over (only then is it pushed to).
#[derive(Debug)]
pub(crate) struct Live {
    tx: Sender<Msg>,
    filter: Filter,
    held: bool,
}

#[derive(Debug, Default)]
pub(crate) struct State {
    script: Mutex<Script>,
    requests: Mutex<Vec<Request>>,
    streams: Mutex<HashMap<(usize, u64), Live>>,
    conns: Mutex<Vec<LocalStream>>,
    accepted: AtomicUsize,
    stopping: AtomicBool,
}

impl State {
    /// Sends `frame` to every held stream that asked for it. One that cannot keep up misses it.
    pub(crate) fn push(&self, frame: &Frame) {
        for live in lock(&self.streams)
            .values()
            .filter(|l| l.held && l.filter.accepts(frame))
        {
            let _ = live.tx.try_send(Msg::Frame(Box::new(frame.clone())));
        }
    }

    fn end_all(&self, error: Option<&str>) {
        for live in lock(&self.streams).values() {
            let _ = live.tx.try_send(Msg::End(error.map(str::to_owned)));
        }
    }
}

/// A running fixture daemon. Dropping it stops it and removes its socket.
#[derive(Debug)]
pub struct FixtureDaemon {
    state: Arc<State>,
    path: PathBuf,
    accept: Option<JoinHandle<()>>,
}

impl FixtureDaemon {
    /// Listens at `path` and plays `script` to whoever connects.
    pub fn start(path: &Path, script: Script) -> io::Result<FixtureDaemon> {
        let listener = LocalListener::bind(path)?;
        let state = Arc::new(State {
            script: Mutex::new(script),
            ..State::default()
        });
        let theirs = Arc::clone(&state);
        let accept = thread::Builder::new()
            .name("fixture-accept".into())
            .spawn(move || accept_loop(&theirs, &listener))?;
        Ok(FixtureDaemon {
            state,
            path: path.to_path_buf(),
            accept: Some(accept),
        })
    }

    /// The socket's path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A client for this daemon, named `test`.
    pub fn client(&self) -> Client {
        Client::socket(&self.path).named("test")
    }

    /// Sends a frame to every held stream that asked for its topic, as a daemon does when something happens.
    pub fn push(&self, frame: &Frame) {
        self.state.push(frame);
    }

    /// A handle that pushes as [`FixtureDaemon::push`] does, from another thread; it stops reaching anything once the
    /// daemon has stopped.
    pub fn pusher(&self) -> Pusher {
        Pusher(Arc::downgrade(&self.state))
    }

    /// Every request received so far, hellos included, in order.
    pub fn requests(&self) -> Vec<Request> {
        lock(&self.state.requests).clone()
    }

    /// What `status` answers now.
    pub fn status(&self) -> Response {
        lock(&self.state.script).status.clone()
    }

    /// How many connections it has accepted.
    pub fn accepted(&self) -> usize {
        self.state.accepted.load(Ordering::SeqCst)
    }

    /// How many streams are running.
    pub fn streams(&self) -> usize {
        lock(&self.state.streams).len()
    }

    /// How many streams have played their script and are held open, so [`FixtureDaemon::push`] reaches them.
    pub fn held(&self) -> usize {
        lock(&self.state.streams)
            .values()
            .filter(|l| l.held)
            .count()
    }

    /// Ends every stream with [`GONE`] and drops every connection, as a daemon that went away does; it still
    /// listens.
    pub fn hang_up(&self) {
        self.state.end_all(Some(GONE));
        for c in lock(&self.state.conns).drain(..) {
            let _ = c.shutdown();
        }
    }

    /// Stops listening, hangs up, and removes the socket.
    pub fn stop(mut self) {
        self.shut();
    }

    fn shut(&mut self) {
        let Some(accept) = self.accept.take() else {
            return;
        };
        self.state.stopping.store(true, Ordering::SeqCst);
        // Wake the accept loop so it sees the flag.
        let _ = LocalStream::connect(&self.path);
        let _ = accept.join();
        self.hang_up();
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Drop for FixtureDaemon {
    fn drop(&mut self) {
        self.shut();
    }
}

fn accept_loop(state: &Arc<State>, listener: &LocalListener) {
    loop {
        let conn = listener.accept();
        if state.stopping.load(Ordering::SeqCst) {
            return;
        }
        let Ok(conn) = conn else { continue };
        let n = state.accepted.fetch_add(1, Ordering::SeqCst) + 1;
        if let Ok(c) = conn.try_clone() {
            lock(&state.conns).push(c);
        }
        let theirs = Arc::clone(state);
        let _ = thread::Builder::new()
            .name(format!("fixture-conn-{n}"))
            .spawn(move || session::serve(&theirs, n, conn));
    }
}

/// A socket path in the system's temp folder that no other test uses (short enough for a Unix socket).
pub fn temp_socket(tag: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!("dzc-{tag}-{}-{n}.sock", std::process::id()))
}

/// Pushes frames to a fixture daemon's held streams from anywhere (a ticker thread, as a daemon sends the meters every
/// second), for as long as the daemon runs.
#[derive(Debug, Clone)]
pub struct Pusher(std::sync::Weak<State>);

impl Pusher {
    /// Sends `frame` as [`FixtureDaemon::push`] does; false once the daemon has stopped.
    pub fn push(&self, frame: &Frame) -> bool {
        match self.0.upgrade() {
            Some(s) if !s.stopping.load(Ordering::SeqCst) => {
                s.push(frame);
                true
            }
            _ => false,
        }
    }
}
