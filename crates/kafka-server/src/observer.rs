//! Watching the broker's traffic: the hook the console's recorder is built on.
//!
//! The connection loop is the only place where a request and its answer are both in scope: the request is decoded,
//! handed to the broker by value, and the answer is encoded and written. An [`Observer`] is told twice for each request:
//! [`Observer::begin`] before the handler, with the request (so it can read what it needs and keep a small token), and
//! [`Observer::end`] after it, with the answer, how long the broker took and how many bytes went back. A connection is
//! told of as well when it opens and when it closes.
//!
//! The observer is optional. With none installed ([`crate::Config::observer`] is `None`) a request costs one `Option`
//! test and nothing else: no allocation, no clock, no connection number. With one, the work is the observer's own and it
//! must be bounded: the calls are made on the connection's task, one request at a time, so a slow observer is a slow
//! connection. Observers read the request and the answer, never copy record bytes, clip what they keep, and never panic
//! (a release build is `panic = "abort"`; in a test build a panic ends only the connection it happened on, because every
//! connection is a task of its own).
//!
//! Nothing here is secret-aware: the request of a `SaslAuthenticate` is handed over like any other, and an observer must
//! not keep what is in it. The recorder of `kafka-console` reads only the fields it names and is tested against planted
//! secrets.

use std::any::Any;
use std::fmt;
use std::net::SocketAddr;
use std::time::Duration;

use kafka_auth::Principal;
use kafka_wire::api::{Request, Response};
use kafka_wire::header::RequestHead;
use kafka_wire::messages::ApiVersionsRequest;
use tokio::time::Instant;

/// A number for one connection, unique for the life of the server (in the order they were accepted).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnId(pub u64);

/// A connection that was just accepted.
#[derive(Debug, Clone)]
pub struct Opened {
    /// Its number.
    pub id: ConnId,
    /// The remote address and port, if it came over a network (none in the tests that use an in-memory pipe).
    pub peer: Option<SocketAddr>,
}

/// What the request was, as far as it could be read.
#[derive(Debug)]
pub enum Asked<'a> {
    /// A request that was decoded.
    Request(&'a Request),
    /// An ApiVersions request: the body, if it could be read (the answer does not need it).
    ApiVersions(Option<&'a ApiVersionsRequest>),
    /// A request whose body was not read: refused on its header alone (a stranger's request before login, a version that
    /// is not served, a body that does not decode).
    Undecoded,
}

/// A request about to be handled.
#[derive(Debug)]
pub struct Call<'a> {
    /// The connection it arrived on.
    pub conn: ConnId,
    /// Who the connection is: nobody in particular until a login says otherwise.
    pub principal: &'a Principal,
    /// Its header: the API, the version, the correlation id and the client's own name for itself.
    pub head: &'a RequestHead,
    /// The request.
    pub asked: Asked<'a>,
    /// The size of the frame it arrived in.
    pub request_bytes: usize,
}

/// How a request ended.
#[derive(Debug)]
pub enum Answer<'a> {
    /// An answer, encoded and about to be written.
    Response(&'a Response),
    /// The request is answered with nothing (a Produce with no acknowledgement asked for).
    Nothing,
    /// The connection was closed instead of answering; the text says why, in words of the server's (it holds nothing a
    /// client sent, except what a decoder says about its own input).
    Closed(&'a str),
}

/// What happened to a request, once the broker has answered it.
#[derive(Debug)]
pub struct Observed<'a> {
    /// The answer.
    pub answer: Answer<'a>,
    /// How long the broker took, from the request being decoded to the answer being ready.
    pub took: Duration,
    /// The size of the answer's frame on the wire; zero if none was written.
    pub response_bytes: usize,
    /// The fault rule that touched the request, if one did (the console's Lab): the answer may be a delay, an error the
    /// rule made up, or a closed connection.
    pub fault: Option<&'a crate::fault::Note>,
}

/// What an observer keeps between [`Observer::begin`] and [`Observer::end`]: whatever it chose to, and when the request
/// began. The payload is the observer's own (one allocation per observed request); the server never looks inside.
pub struct Pending {
    began: Instant,
    payload: Box<dyn Any + Send>,
}

impl Pending {
    /// A token that carries `payload` to the matching [`Observer::end`].
    #[must_use]
    pub fn new<T: Any + Send>(payload: T) -> Self {
        Self {
            began: Instant::now(),
            payload: Box::new(payload),
        }
    }

    /// The payload, if it is a `T`; `None` for a token that another observer made.
    #[must_use]
    pub fn into_payload<T: Any>(self) -> Option<T> {
        self.payload.downcast::<T>().ok().map(|b| *b)
    }

    /// Since the token was made.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.began.elapsed()
    }
}

impl fmt::Debug for Pending {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The payload is the observer's and may hold request facts: it is not printed.
        f.debug_struct("Pending")
            .field("elapsed", &self.elapsed())
            .finish_non_exhaustive()
    }
}

/// Something that watches the broker's connections and requests. See the module's rules: bounded work, no panics, no
/// secrets kept.
pub trait Observer: Send + Sync + fmt::Debug {
    /// A connection was accepted.
    fn opened(&self, conn: &Opened);

    /// A request is about to be handled. The token goes back to [`Observer::end`].
    fn begin(&self, call: &Call<'_>) -> Pending;

    /// The request ended: answered, answered with nothing, or the connection was closed instead.
    fn end(&self, pending: Pending, seen: &Observed<'_>);

    /// The connection is gone. `why` is the server's own sentence for it (the client closed, it was idle for too long, a
    /// login was refused...).
    fn closed(&self, conn: ConnId, why: &str);
}

/// What one connection holds of an installed observer: built when the connection is accepted and only if there is an
/// observer, so a connection without one carries a single `None`.
pub(crate) struct Watch {
    pub(crate) observer: std::sync::Arc<dyn Observer>,
    pub(crate) conn: ConnId,
}

impl fmt::Debug for Watch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Watch").field("conn", &self.conn).finish()
    }
}

impl Watch {
    pub(crate) fn begin(
        &self,
        principal: &Principal,
        head: &RequestHead,
        asked: Asked<'_>,
        request_bytes: usize,
    ) -> Pending {
        self.observer.begin(&Call {
            conn: self.conn,
            principal,
            head,
            asked,
            request_bytes,
        })
    }

    pub(crate) fn end(
        &self,
        pending: Pending,
        answer: Answer<'_>,
        response_bytes: usize,
        fault: Option<&crate::fault::Note>,
    ) {
        let took = pending.elapsed();
        self.observer.end(
            pending,
            &Observed {
                answer,
                took,
                response_bytes,
                fault,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_carries_its_payload_back_and_only_as_its_own_type() {
        let token = Pending::new(String::from("kept"));
        assert_eq!(token.into_payload::<u32>(), None);
        let token = Pending::new(String::from("kept"));
        assert_eq!(token.into_payload::<String>().as_deref(), Some("kept"));
    }

    #[test]
    fn a_token_does_not_print_what_it_carries() {
        let token = Pending::new(String::from("a-secret-the-observer-kept"));
        assert!(!format!("{token:?}").contains("secret"));
    }
}
