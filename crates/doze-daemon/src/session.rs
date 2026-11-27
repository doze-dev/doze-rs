//! One client connection (Go's `Server.handleConn`): the hello, refused in Go's words on a mismatch, then requests
//! until the client goes. Everything is a task on the daemon's runtime: the connection's reader, its writer, and each
//! request and stream; `cancel` ends the stream it names.
//!
//! Frames reach the client through one writer task fed by a bounded queue ([`OUTBOX`] frames), so they go out whole
//! and in the order they were queued, and a client that reads slowly (or not at all: `doze logs -f | less`, paused)
//! holds back only the tasks writing to it, which wait for room; never a runtime worker. It is not disconnected for it:
//! what one such client can hold is bounded by the queue, [`crate::Limits::in_flight`] and
//! [`crate::Limits::connections`]. A client that sends no hello within [`crate::Limits::hello`] is closed, so silent
//! sockets cannot hold the connection slots.

use crate::{Daemon, lock, topics, unary};
use doze_control::codec::{self, CodecError, Decoder};
use doze_control::{Frame, Hello, Op, PROTO_VERSION, Request, Response};
use doze_os::local_async::{ReadHalf, WriteHalf};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, watch};

/// How many frames wait for one client before the tasks writing to it wait too.
const OUTBOX: usize = 256;

/// The write half's queue, shared by the tasks answering the connection.
pub(crate) type Writer = mpsc::Sender<Out>;

/// What the writer task is handed.
#[derive(Debug)]
pub(crate) enum Out {
    /// One encoded frame, with what decides whether it still goes.
    Frame {
        id: u64,
        /// A stream's frame (not an answer): dropped once its stream has sent its `end`.
        streamed: bool,
        end: bool,
        line: Vec<u8>,
    },
    /// A new stream reuses `id`: its frames go again.
    Reopen(u64),
}

/// What ends each stream of this connection, by request id.
type Streams = Arc<Mutex<BTreeMap<u64, watch::Sender<bool>>>>;

fn gone() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "the client has gone")
}

/// Queues `f` for the client, waiting while its queue is full; `Err` once the connection is gone.
pub(crate) async fn send(w: &Writer, f: &Frame) -> io::Result<()> {
    let line = codec::encode(f).map_err(io::Error::other)?;
    let out = Out::Frame {
        id: f.id,
        streamed: f.id != 0 && f.response.is_none(),
        end: f.end,
        line,
    };
    w.send(out).await.map_err(|_| gone())
}

/// The writer task: each frame in the order queued, one write per frame. Request ids whose stream has ended get nothing
/// more: a topic's task can be part-way through a reading (several frames queued with no await between them) when its
/// stream is cancelled; without this its last frames could follow the `end`, and a client reading one line per
/// request would take one for the answer to its next request.
async fn write_out(mut out: WriteHalf, mut rx: mpsc::Receiver<Out>) {
    let mut ended: BTreeSet<u64> = BTreeSet::new();
    while let Some(o) = rx.recv().await {
        let line = match o {
            Out::Reopen(id) => {
                ended.remove(&id);
                continue;
            }
            Out::Frame {
                id,
                streamed,
                end,
                line,
            } => {
                if streamed && ended.contains(&id) {
                    continue;
                }
                if end {
                    ended.insert(id);
                }
                line
            }
        };
        if out.write_all(&line).await.is_err() {
            break;
        }
    }
    // Closing the queue tells the reader and every stream that the client is gone.
    rx.close();
    let _ = out.shutdown().await;
}

/// The connection's read half and what it has read but not yet decoded.
struct Reader {
    r: ReadHalf,
    dec: Decoder,
}

impl Reader {
    // fuzz:parser fuzz_session
    /// The next request; `None` once the client has gone (the bytes after the hello: `fuzz_session` sends any).
    async fn next(&mut self) -> Option<Result<Request, CodecError>> {
        let mut buf = [0u8; 8192];
        loop {
            match self.dec.next_frame::<Request>() {
                Ok(Some(req)) => return Some(Ok(req)),
                Ok(None) => {}
                Err(e) => return Some(Err(e)),
            }
            match self.r.read(&mut buf).await {
                Ok(0) | Err(_) => return None,
                Ok(n) => self.dec.push(buf.get(..n).unwrap_or_default()),
            }
        }
    }
}

/// Go's words for a client that speaks another protocol, or none.
pub(crate) fn mismatch(hello: Option<&Hello>) -> String {
    let spoke = hello.map_or_else(
        || "no protocol version".to_owned(),
        |h| format!("protocol {}", h.proto),
    );
    format!(
        "this daemon speaks control protocol {PROTO_VERSION} and the client sent {spoke} — they are different versions of doze; run `doze down` and try again"
    )
}

fn refusal(error: String) -> Frame {
    Frame {
        error,
        ..Frame::default()
    }
}

/// Reads the hello and answers it; `false` if the connection is refused or gone.
async fn greet(w: &Writer, reader: &mut Reader) -> bool {
    let first = match reader.next().await {
        Some(Ok(r)) => r,
        Some(Err(e)) => {
            let _ = send(w, &refusal(format!("bad request: {e}"))).await;
            return false;
        }
        None => return false,
    };
    match &first.hello {
        Some(h) if h.proto == PROTO_VERSION => {
            if !h.client.is_empty() {
                tracing::debug!(client = %h.client, "control: hello");
            }
            let hello = Hello {
                proto: PROTO_VERSION,
                client: String::new(),
            };
            let frame = Frame {
                hello: Some(hello),
                ..Frame::default()
            };
            send(w, &frame).await.is_ok()
        }
        other => {
            let _ = send(w, &refusal(mismatch(other.as_ref()))).await;
            false
        }
    }
}

/// How long telling a turned-away client why may take: one short frame into an empty socket, so only a client that
/// is not there at all waits it out.
const TURN_AWAY: Duration = Duration::from_millis(200);

/// A connection past the cap: told so in place of the hello's answer, and closed. It holds no connection slot. What the
/// client sends meanwhile (its hello) is read and dropped for a moment first: closing at once would fail the client's
/// own write of its hello, before it could read why.
pub(crate) async fn turn_away(mut r: ReadHalf, mut w: WriteHalf, open: usize) {
    let f = refusal(format!(
        "the daemon is serving {open} control connections, the most it takes; close one (a `doze logs -f`, a second app) and try again"
    ));
    if let Ok(line) = codec::encode(&f) {
        let _ = tokio::time::timeout(TURN_AWAY, w.write_all(&line)).await;
    }
    let _ = tokio::time::timeout(TURN_AWAY, w.shutdown()).await;
    let mut sink = [0u8; 1024];
    let _ = tokio::time::timeout(TURN_AWAY, async {
        while matches!(r.read(&mut sink).await, Ok(n) if n > 0) {}
    })
    .await;
}

/// Serves one connection until its client goes.
pub(crate) async fn serve(d: Daemon, r: ReadHalf, w: WriteHalf) {
    let limits = d.inner.limits;
    let (tx, rx) = mpsc::channel(OUTBOX);
    let writer = tokio::spawn(write_out(w, rx));
    let mut reader = Reader {
        r,
        dec: Decoder::new(),
    };
    // A client says hello at once (doze-control's waits HELLO_TIMEOUT for the answer): one that does not is closed.
    let greeted = tokio::time::timeout(limits.hello, greet(&tx, &mut reader))
        .await
        .unwrap_or(false);
    if greeted {
        let streams: Streams = Arc::default();
        let in_flight = Arc::new(Semaphore::new(limits.in_flight));
        loop {
            let next = tokio::select! {
                n = reader.next() => n,
                // The writer could not write to the client: it has gone.
                () = tx.closed() => None,
            };
            match next {
                Some(Ok(req)) => {
                    if dispatch(&d, &tx, &streams, &in_flight, req).await.is_err() {
                        break;
                    }
                }
                Some(Err(e)) => {
                    let _ = send(&tx, &refusal(format!("bad request: {e}"))).await;
                    break;
                }
                None => break,
            }
        }
        // The client went away: the streams it started end with it (unary requests run on).
        for end in lock(&streams).values() {
            let _ = end.send(true);
        }
    }
    // The writer ends on its own once every task holding the queue has let it go: what they queued still goes.
    drop(writer);
}

/// Starts `req` as a task of its own; `Err` once the connection is gone.
async fn dispatch(
    d: &Daemon,
    w: &Writer,
    streams: &Streams,
    in_flight: &Arc<Semaphore>,
    req: Request,
) -> io::Result<()> {
    if req.known_op() == Some(Op::Cancel) {
        if let Some(end) = lock(streams).get(&req.id) {
            let _ = end.send(true);
        }
        return Ok(());
    }
    let Ok(permit) = Arc::clone(in_flight).try_acquire_owned() else {
        let cap = d.inner.limits.in_flight;
        let busy = Response {
            error: format!(
                "this connection has {cap} requests and streams running, the most the daemon runs for one client; wait for one to end, or cancel a stream"
            ),
            ..Response::default()
        };
        return send(w, &reply(req.id, busy)).await;
    };
    let (d, w) = (d.clone(), w.clone());
    if !req.is_stream() {
        spawn_unary(w, permit, req.id, move || async move {
            unary::answer(&d, &req).await
        });
        return Ok(());
    }
    let (tx, rx) = watch::channel(false);
    let taken = {
        let mut open = lock(streams);
        let taken = open.contains_key(&req.id);
        if !taken {
            open.insert(req.id, tx);
        }
        taken
    };
    if taken {
        let r = Response {
            error: format!(
                "request id {} is already an open stream on this connection; cancel it, or use another id",
                req.id
            ),
            ..Response::default()
        };
        return send(&w, &reply(req.id, r)).await;
    }
    // An id used again for a new stream (its last one has ended) writes again.
    w.send(Out::Reopen(req.id)).await.map_err(|_| gone())?;
    let streams = Arc::clone(streams);
    let id = req.id;
    tokio::spawn(async move {
        let w2 = w.clone();
        let ran = caught(async move { topics::run(&d, &w2, &req, rx).await }).await;
        let last = ran.unwrap_or_else(|error| {
            Some(Frame {
                id,
                end: true,
                error,
                ..Frame::default()
            })
        });
        // No longer open before its last frame goes: a client that has read the `end` can use the id again.
        lock(&streams).remove(&id);
        if let Some(f) = last {
            let _ = send(&w, &f).await;
        }
        drop(permit);
    });
    Ok(())
}

/// Answers a unary request on a task of its own; the permit goes once the answer is queued, whatever happened.
fn spawn_unary<F, Fut>(w: Writer, permit: OwnedSemaphorePermit, id: u64, answer: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Response> + Send + 'static,
{
    tokio::spawn(async move {
        let response = caught(answer()).await.unwrap_or_else(|error| Response {
            error,
            ..Response::default()
        });
        let _ = send(&w, &reply(id, response)).await;
        drop(permit);
    });
}

/// Runs `fut` as a task of its own and waits for it; `Err` with what to tell the client if it panicked (a release build
/// aborts on a panic instead: this is for every other build, tests included).
async fn caught<T: Send + 'static>(
    fut: impl std::future::Future<Output = T> + Send + 'static,
) -> Result<T, String> {
    tokio::spawn(fut).await.map_err(|e| {
        tracing::error!("control: a request's handler failed: {e}");
        "the daemon failed while answering this request (a bug; its log says more)".to_owned()
    })
}

pub(crate) fn reply(id: u64, response: Response) -> Frame {
    Frame {
        id,
        response: Some(response),
        ..Frame::default()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)] // test code
mod tests {
    use super::*;

    async fn answer_of(rx: &mut mpsc::Receiver<Out>) -> Frame {
        match rx.recv().await {
            Some(Out::Frame { line, .. }) => serde_json::from_slice(&line).unwrap(),
            other => panic!("{other:?}"),
        }
    }

    /// A handler that panics is answered with an error, and its permit is let go.
    #[tokio::test]
    async fn a_panicking_handler_is_answered_and_lets_its_permit_go() {
        let room = Arc::new(Semaphore::new(1));
        let (tx, mut rx) = mpsc::channel(4);
        let permit = Arc::clone(&room).try_acquire_owned().unwrap();
        spawn_unary(tx, permit, 7, || async { panic!("a bug") });
        let f = answer_of(&mut rx).await;
        assert_eq!(f.id, 7);
        let r = f.response.unwrap();
        assert!(!r.ok && r.error.contains("failed while answering"), "{r:?}");
        let again =
            tokio::time::timeout(Duration::from_secs(5), Arc::clone(&room).acquire_owned()).await;
        assert!(again.is_ok(), "the permit was kept");
    }

    /// A handler that answers with an error lets its permit go too.
    #[tokio::test]
    async fn a_failed_request_lets_its_permit_go() {
        let room = Arc::new(Semaphore::new(1));
        let (tx, mut rx) = mpsc::channel(4);
        let permit = Arc::clone(&room).try_acquire_owned().unwrap();
        spawn_unary(tx, permit, 8, || async {
            Response {
                error: "no".into(),
                ..Response::default()
            }
        });
        assert_eq!(answer_of(&mut rx).await.response.unwrap().error, "no");
        let again =
            tokio::time::timeout(Duration::from_secs(5), Arc::clone(&room).acquire_owned()).await;
        assert!(again.is_ok(), "the permit was kept");
    }
}
