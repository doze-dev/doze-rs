//! Which connections are in use and which are only open.
//!
//! A browser leaves connections behind: Firefox and WebKit keep the connections of a page they have closed, and open spare ones
//! to a host they expect to need, and hold them until the server ends them. Counted as they are, a run of quick page loads
//! fills the cap with connections that say nothing, and the next page's own requests are answered 503 (Firefox shows that as a
//! script or a style that "is disallowed by its MIME type"). So the cap is held against connections that are in use: when a
//! new connection finds it full, the connection that has been idle longest is given up for it, and only when none is idle is
//! the new one refused.
//!
//! A connection is in use from the head of a request to the end of its answer, a stream's last event included: [`Tracked`]
//! holds a [`Busy`] for as long as the answer's body lives.

use std::collections::HashMap;
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};
use std::time::Instant;

use axum::body::{Body, Bytes};
use hyper::body::{Frame, Incoming, SizeHint};
use hyper::service::Service;
use hyper::{Request, Response};
use hyper_util::service::TowerToHyperService;
use tokio_util::sync::CancellationToken;

/// One connection: how many requests are in hand, and since when it has had none.
#[derive(Debug)]
pub struct Conn {
    started: Instant,
    busy: AtomicUsize,
    /// Milliseconds after `started` at which the last request ended (0 for one that has had none).
    idle_since: AtomicU64,
    /// Given up for another connection: not given up twice.
    closing: AtomicBool,
    /// Ends the connection once the request in hand (if any) is answered.
    pub close: CancellationToken,
}

impl Conn {
    fn now(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// A request begins.
    fn begin(self: &Arc<Self>) -> Busy {
        self.busy.fetch_add(1, Ordering::AcqRel);
        Busy(Arc::clone(self))
    }
}

/// A request in hand; dropped when its answer is over.
#[derive(Debug)]
pub struct Busy(Arc<Conn>);

impl Drop for Busy {
    fn drop(&mut self) {
        if self.0.busy.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.0.idle_since.store(self.0.now(), Ordering::Release);
        }
    }
}

/// Every connection that is open.
#[derive(Debug, Default)]
pub struct Conns {
    next: AtomicU64,
    open: Mutex<HashMap<u64, Arc<Conn>>>,
}

/// A connection in the set; leaves it when dropped.
#[derive(Debug)]
pub struct Member {
    conns: Arc<Conns>,
    id: u64,
    pub conn: Arc<Conn>,
}

impl Drop for Member {
    fn drop(&mut self) {
        self.conns
            .open
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.id);
    }
}

impl Conns {
    /// An empty set.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::default()
    }

    /// Takes a new connection into the set. It starts idle.
    pub fn join(self: &Arc<Self>) -> Member {
        let conn = Arc::new(Conn {
            started: Instant::now(),
            busy: AtomicUsize::new(0),
            idle_since: AtomicU64::new(0),
            closing: AtomicBool::new(false),
            close: CancellationToken::new(),
        });
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.open
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, Arc::clone(&conn));
        Member {
            conns: Arc::clone(self),
            id,
            conn,
        }
    }

    /// Gives up the connection that has been idle longest (it is closed at once). Whether a place is on its way: one was given
    /// up now, or one given up before is still closing.
    pub fn give_up_one_idle(&self) -> bool {
        let open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        // The idle time of a connection is read once: a request may begin as we look, and then it is answered before the
        // connection closes (a connection that is closed while it is in use finishes the request in hand).
        let longest = open
            .values()
            .filter(|c| c.busy.load(Ordering::Acquire) == 0 && !c.closing.load(Ordering::Acquire))
            .min_by_key(|c| c.idle_since.load(Ordering::Acquire));
        match longest {
            Some(c) => {
                c.closing.store(true, Ordering::Release);
                c.close.cancel();
                true
            }
            None => open.values().any(|c| c.closing.load(Ordering::Acquire)),
        }
    }
}

/// The console's service for one connection: it counts the requests in hand, to the end of their answers.
#[derive(Debug)]
pub struct Tracked {
    pub inner: TowerToHyperService<axum::Router>,
    pub conn: Arc<Conn>,
}

type Answer = Result<Response<Body>, Infallible>;

impl Service<Request<Incoming>> for Tracked {
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Answer> + Send>>;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        let busy = self.conn.begin();
        let answer = self.inner.call(req);
        Box::pin(async move {
            let res = answer.await?;
            Ok(res.map(|inner| Body::new(Held { inner, _busy: busy })))
        })
    }
}

/// An answer's body, with the request it answers still counted until the body is done with.
struct Held {
    inner: Body,
    _busy: Busy,
}

impl hyper::body::Body for Held {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
        Pin::new(&mut self.inner).poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}
