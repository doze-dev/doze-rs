//! The blocking client: a [`Client`] dials, a [`Connection`] carries calls and streams, a [`Follow`] keeps a
//! subscription alive across daemon restarts. Frames reach their consumer over `async_channel`, so a GPUI entity can
//! await them while a CLI blocks on them.

pub(crate) mod connection;
mod follow;
mod ops;
mod stream;
pub(crate) mod wait;

pub use connection::{Connection, HELLO_TIMEOUT};
pub use follow::{Backoff, Follow, FollowEvent};
pub use stream::{Pending, Stream, StreamEvent};

use crate::conn::{Dialer, SocketDialer};
use crate::{Error, Request, Response, Subscription};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// How long a unary call waits by default (Go's client gives up after a minute).
pub const CALL_TIMEOUT: Duration = Duration::from_secs(60);

/// Dials one daemon.
#[derive(Debug, Clone)]
pub struct Client {
    dialer: Arc<dyn Dialer>,
    name: String,
    timeout: Option<Duration>,
}

impl Client {
    /// A client for whatever `dialer` connects to, named `cli` in the hello as Go's is.
    pub fn new(dialer: impl Dialer) -> Client {
        Client {
            dialer: Arc::new(dialer),
            name: "cli".to_owned(),
            timeout: Some(CALL_TIMEOUT),
        }
    }

    /// A client for the control socket at `path`.
    pub fn socket(path: impl Into<PathBuf>) -> Client {
        Client::new(SocketDialer::new(path))
    }

    /// Names this client in the hello (`app`, `cli`, `dash`), for the daemon's log.
    #[must_use]
    pub fn named(mut self, name: &str) -> Client {
        name.clone_into(&mut self.name);
        self
    }

    /// How long unary calls wait (`None`: for ever) and how long a stream waits for its acknowledgement.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Option<Duration>) -> Client {
        self.timeout = timeout;
        self
    }

    /// Connects and exchanges hellos. A daemon that speaks another protocol version is an error that says so.
    pub fn dial(&self) -> Result<Connection, Error> {
        Connection::open(self.dialer.as_ref(), &self.name, self.timeout)
    }

    /// Whether something is listening (no hello is exchanged).
    pub fn available(&self) -> bool {
        match self.dialer.dial() {
            Ok(c) => {
                let _ = c.shutdown();
                true
            }
            Err(_) => false,
        }
    }

    /// Dials, asks, and hangs up.
    pub fn call(&self, req: Request) -> Result<Response, Error> {
        self.dial()?.call(req)
    }

    /// Follows `sub` on its own thread, reconnecting with `backoff` whenever the daemon goes away.
    pub fn follow(&self, sub: Subscription, backoff: Backoff) -> Follow {
        Follow::start(self.clone(), sub, backoff)
    }
}
