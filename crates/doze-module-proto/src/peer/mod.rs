//! The peer: one side of a module channel, running protocol 2. Both the host and the module use it.
//!
//! Outgoing: [`Peer::call`] (one answer), [`Peer::stream`] (items under credit), [`Peer::notify`] (one-way event or
//! log). Incoming calls go to the [`Handler`] given at start. The method table ([`crate::method`]) is enforced here: a
//! call this build does not know, a call from the wrong side, one the other side's capabilities or version do not
//! allow, and a callback outside the call it names are all refused with a structured error.

mod hello;
mod incoming;
mod inner;
mod io;
mod serve;
mod stream;

pub use hello::{accept_module, connect_host};
pub use serve::{Cancel, Incoming, Responder};
pub use stream::Stream;

use crate::flow::Inbound;
use crate::frame::{self, Kind};
use crate::method::{Direction, Method, Shape};
use crate::msg::error::{Error, ErrorKind};
use crate::msg::{self, CallHead};
use doze_os::channel::Passable;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{Notify, mpsc, oneshot};

/// Serves incoming calls. It owns the [`Incoming`] and answers through its responder.
pub type Handler = Arc<dyn Fn(Incoming) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// Receives one-way frames (events and logs): kind and body.
pub type Notices = Arc<dyn Fn(Kind, Vec<u8>) + Send + Sync>;

/// Which side of the channel this peer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Host,
    Module,
}

/// Options of one outgoing call.
#[derive(Debug, Default)]
pub struct CallOpts {
    pub deadline: Option<Duration>,
    /// For a callback: the incoming call it serves (`Incoming::id`).
    pub for_call: Option<u32>,
    /// A connection to hand over with the call.
    pub handle: Option<Passable>,
}

/// A stream's send credit and the signal that more arrived.
pub(crate) type Credit = Arc<(Mutex<crate::flow::Outbound>, Notify)>;

pub(crate) enum Out {
    Frame(Vec<u8>),
    Pass {
        id: u32,
        method: u16,
        body: Vec<u8>,
        handle: Passable,
    },
}

enum Pending {
    Unary(Method, oneshot::Sender<Result<Vec<u8>, Error>>),
    Stream(
        Method,
        mpsc::UnboundedSender<Result<Vec<u8>, Error>>,
        Inbound,
    ),
}

pub(crate) struct Inner {
    side: Side,
    out: mpsc::UnboundedSender<Out>,
    pending: Mutex<HashMap<u32, Pending>>,
    serving: Mutex<HashMap<u32, serve::Cancel>>,
    credits: Mutex<HashMap<u32, Credit>>,
    next: AtomicU32,
    closed: AtomicBool,
    closed_signal: Notify,
    /// The agreed minor (set after hello) and the module's capabilities.
    minor: AtomicU16,
    caps: AtomicU64,
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inner")
            .field("side", &self.side)
            .finish_non_exhaustive()
    }
}

/// One side of a running channel. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Peer {
    inner: Arc<Inner>,
}

fn internal(e: impl std::fmt::Display) -> Error {
    Error::new(ErrorKind::Internal, e.to_string())
}

impl Peer {
    /// Whether this side may make `method` now: direction, capability, agreed version.
    fn check(&self, method: Method) -> Result<(), Error> {
        let i = &self.inner;
        let ours = match i.side {
            Side::Host => Direction::HostToModule,
            Side::Module => Direction::ModuleToHost,
        };
        if method.direction() != ours {
            return Err(Error::new(
                ErrorKind::Unsupported,
                format!("{method:?} is not called from this side"),
            ));
        }
        let cap = method.capability();
        if cap != 0 && i.caps.load(Ordering::SeqCst) & cap == 0 {
            return Err(Error::new(
                ErrorKind::Unsupported,
                format!("the module does not offer {method:?}"),
            ));
        }
        let agreed = crate::version::Version {
            major: 2,
            minor: i.minor.load(Ordering::SeqCst),
        };
        if !agreed.has(method.since()) {
            return Err(Error::new(
                ErrorKind::Unsupported,
                format!("{method:?} needs a newer protocol minor"),
            ));
        }
        Ok(())
    }

    fn send_call<A: minicbor::Encode<()>>(
        &self,
        method: Method,
        args: &A,
        opts: CallOpts,
        p: impl FnOnce(u32) -> Pending,
    ) -> Result<u32, Error> {
        self.check(method)?;
        let id = self.inner.next.fetch_add(1, Ordering::Relaxed);
        let head = CallHead {
            deadline_ms: opts
                .deadline
                .map(|d| u32::try_from(d.as_millis()).unwrap_or(u32::MAX)),
            for_call: opts.for_call,
        };
        let body = msg::encode_call(head, args).map_err(internal)?;
        let out = match opts.handle {
            Some(handle) => Out::Pass {
                id,
                method: method as u16,
                body,
                handle,
            },
            None => Out::Frame(
                frame::encode(id, Kind::Call, method as u16, &body, None).map_err(internal)?,
            ),
        };
        {
            let mut pending = self.inner.pending.lock().map_err(internal)?;
            if self.inner.is_closed() {
                return Err(Error::new(ErrorKind::Unavailable, "the connection ended"));
            }
            pending.insert(id, p(id));
        }
        self.inner.queue(out)?;
        Ok(id)
    }

    /// Makes a call and waits for its answer (or its deadline).
    pub async fn call<A, R>(&self, method: Method, args: &A, opts: CallOpts) -> Result<R, Error>
    where
        A: minicbor::Encode<()>,
        R: for<'b> minicbor::Decode<'b, ()>,
    {
        if method.shape() != Shape::Unary {
            return Err(Error::new(
                ErrorKind::Internal,
                format!("{method:?} is a stream"),
            ));
        }
        let deadline = opts.deadline;
        let (tx, rx) = oneshot::channel();
        let id = self.send_call(method, args, opts, |_| Pending::Unary(method, tx))?;
        let answer = match deadline {
            Some(d) => match tokio::time::timeout(d, rx).await {
                Ok(a) => a,
                Err(_) => {
                    self.cancel(id);
                    return Err(Error::new(
                        ErrorKind::TimedOut,
                        format!("{method:?} did not answer within {d:?}"),
                    ));
                }
            },
            None => rx.await,
        };
        let bytes =
            answer.map_err(|_| Error::new(ErrorKind::Unavailable, "the connection ended"))??;
        msg::decode(&bytes)
            .map_err(|e| Error::new(ErrorKind::Internal, format!("answer of {method:?}: {e}")))
    }

    /// Opens a stream; read it with [`Stream::next`]. Dropping the stream cancels it.
    pub fn stream<A: minicbor::Encode<()>>(
        &self,
        method: Method,
        args: &A,
        opts: CallOpts,
    ) -> Result<Stream, Error> {
        if method.shape() != Shape::Stream {
            return Err(Error::new(
                ErrorKind::Internal,
                format!("{method:?} is not a stream"),
            ));
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let id = self.send_call(method, args, opts, |_| {
            Pending::Stream(method, tx, Inbound::new())
        })?;
        Ok(Stream {
            id,
            rx,
            peer: self.clone(),
            done: false,
        })
    }

    /// Sends a one-way event or log record (module side).
    pub fn notify<T: minicbor::Encode<()>>(&self, kind: Kind, v: &T) -> Result<(), Error> {
        let body = msg::encode(v).map_err(internal)?;
        self.inner.queue(Out::Frame(
            frame::encode(0, kind, 0, &body, None).map_err(internal)?,
        ))
    }

    fn cancel(&self, id: u32) {
        if let Ok(mut p) = self.inner.pending.lock() {
            p.remove(&id);
        }
        if let Ok(b) = frame::encode(id, Kind::Cancel, 0, &[], None) {
            let _ = self.inner.queue(Out::Frame(b));
        }
    }

    /// Whether the connection has ended.
    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }

    /// Resolves when the connection has ended.
    pub async fn closed(&self) {
        loop {
            let wait = self.inner.closed_signal.notified();
            if self.inner.is_closed() {
                return;
            }
            wait.await;
        }
    }
}
