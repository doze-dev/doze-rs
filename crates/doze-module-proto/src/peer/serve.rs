//! The callee's side of a call: what a handler receives ([`Incoming`]), how it answers ([`Responder`]), and how it
//! learns that the caller gave up or the deadline passed ([`Cancel`]).

use super::{Inner, Out};
use crate::frame::{self, Kind};
use crate::method::Method;
use crate::msg::error::{Error, ErrorKind};
use crate::msg::{self, CallHead};
use doze_os::channel::Passable;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Notify;

/// Fires when a call is cancelled: by the caller, by its deadline, or because the connection ended.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<(AtomicBool, Notify)>);

impl Cancel {
    pub(super) fn fire(&self) {
        self.0.0.store(true, Ordering::SeqCst);
        self.0.1.notify_waiters();
    }

    /// Whether the call has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.0.0.load(Ordering::SeqCst)
    }

    /// Resolves once the call is cancelled.
    pub async fn cancelled(&self) {
        loop {
            let wait = self.0.1.notified();
            if self.is_cancelled() {
                return;
            }
            wait.await;
        }
    }
}

/// A call to serve.
#[derive(Debug)]
pub struct Incoming {
    pub method: Method,
    pub head: CallHead,
    pub(super) args: Vec<u8>,
    /// A connection handed over with the call (`wire.accept`).
    pub handle: Option<Passable>,
    pub responder: Responder,
    pub cancel: Cancel,
}

impl Incoming {
    /// The call's id: name it in `for_call` when making a callback for it.
    pub fn id(&self) -> u32 {
        self.responder.id
    }

    /// Decodes the arguments.
    pub fn args<'b, T: minicbor::Decode<'b, ()>>(&'b self) -> Result<T, Error> {
        msg::decode(&self.args).map_err(|e| {
            Error::new(
                ErrorKind::Invalid,
                format!("arguments of {:?}: {e}", self.method),
            )
        })
    }
}

/// Answers one call. A unary call is answered once with [`ok`](Responder::ok) or [`err`](Responder::err); a stream
/// sends [`item`](Responder::item)s and finishes with [`end`](Responder::end) or [`err`](Responder::err).
#[derive(Debug)]
pub struct Responder {
    pub(super) id: u32,
    pub(super) inner: Arc<Inner>,
    /// Stream credit (streams only), and the signal that more arrived.
    pub(super) credit: Option<super::Credit>,
    pub(super) answered: bool,
}

impl Responder {
    fn send(&self, kind: Kind, body: &[u8]) -> Result<(), Error> {
        let bytes = frame::encode(self.id, kind, 0, body, None)
            .map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))?;
        self.inner.queue(Out::Frame(bytes))
    }

    fn done(&mut self) {
        self.answered = true;
        self.inner.finish_serving(self.id);
    }

    /// Answers with a value.
    pub fn ok<T: minicbor::Encode<()>>(mut self, v: &T) -> Result<(), Error> {
        let body = msg::encode(v).map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))?;
        let r = self.send(Kind::Reply, &body);
        self.done();
        r
    }

    /// Answers with an error.
    pub fn err(mut self, e: &Error) -> Result<(), Error> {
        let body = msg::encode(e).map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))?;
        let r = self.send(Kind::Error, &body);
        self.done();
        r
    }

    /// Sends one stream item, waiting for credit from the receiver first.
    pub async fn item<T: minicbor::Encode<()>>(&self, v: &T) -> Result<(), Error> {
        let credit = self
            .credit
            .as_ref()
            .ok_or_else(|| Error::new(ErrorKind::Internal, "item on a unary call"))?;
        loop {
            let more = credit.1.notified();
            let took = credit.0.lock().map(|mut c| c.take()).unwrap_or(false);
            if took {
                break;
            }
            if self.inner.is_closed() {
                return Err(Error::new(ErrorKind::Cancelled, "the connection ended"));
            }
            more.await;
        }
        let body = msg::encode(v).map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))?;
        self.send(Kind::Item, &body)
    }

    /// Ends a stream.
    pub fn end(mut self) -> Result<(), Error> {
        let r = self.send(Kind::End, &[]);
        self.done();
        r
    }
}

impl Drop for Responder {
    /// A handler that returns without answering would leave the caller waiting for its deadline: answer for it.
    fn drop(&mut self) {
        if self.answered {
            return;
        }
        let e = Error::new(
            ErrorKind::Internal,
            "the call was dropped without an answer",
        );
        if let Ok(body) = msg::encode(&e) {
            let _ = self.send(Kind::Error, &body);
        }
        self.done();
    }
}
