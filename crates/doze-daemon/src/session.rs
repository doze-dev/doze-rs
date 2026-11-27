//! One client connection (Go's `Server.handleConn`): the hello, refused in Go's words on a mismatch, then requests
//! until the client goes. Everything is a task on the daemon's runtime: the connection's reader, its writer, and each
//! request and stream; `cancel` ends the stream it names.
//!
//! Frames reach the client through one writer task fed by a bounded queue ([`OUTBOX`] frames), so they go out whole
//! and in the order they were queued, and a client that reads slowly holds back only the tasks writing to it (they
//! wait for room), never a runtime worker. A client that takes nothing for [`crate::Limits::write_stall`] while frames
//! wait is disconnected: its streams end, its unary requests run on.

use crate::{Daemon, lock, topics, unary};
use doze_control::codec::{self, CodecError, Decoder};
use doze_control::{Frame, Hello, Op, PROTO_VERSION, Request, Response};
use doze_os::local_async::{ReadHalf, WriteHalf};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Semaphore, mpsc, watch};

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
async fn write_out(mut out: WriteHalf, mut rx: mpsc::Receiver<Out>, stall: Duration) {
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
        match tokio::time::timeout(stall, out.write_all(&line)).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => break,
            Err(_) => {
                tracing::warn!(
                    "control: a client read nothing for {stall:?} with frames waiting; disconnecting it"
                );
                break;
            }
        }
    }
    // Closing the queue tells the reader and every stream that the client is gone.
    rx.close();
    let _ = tokio::time::timeout(stall, out.shutdown()).await;
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

/// A connection past the cap: told so in place of the hello's answer, and closed.
pub(crate) async fn turn_away(mut w: WriteHalf, open: usize) {
    let f = refusal(format!(
        "the daemon is serving {open} control connections, the most it takes; close one (a `doze logs -f`, a second app) and try again"
    ));
    if let Ok(line) = codec::encode(&f) {
        let _ = tokio::time::timeout(Duration::from_secs(1), w.write_all(&line)).await;
    }
    let _ = tokio::time::timeout(Duration::from_secs(1), w.shutdown()).await;
}

/// Serves one connection until its client goes (or is disconnected for not reading).
pub(crate) async fn serve(d: Daemon, r: ReadHalf, w: WriteHalf) {
    let limits = d.inner.limits;
    let (tx, rx) = mpsc::channel(OUTBOX);
    let writer = tokio::spawn(write_out(w, rx, limits.write_stall));
    let mut reader = Reader {
        r,
        dec: Decoder::new(),
    };
    if greet(&tx, &mut reader).await {
        let streams: Streams = Arc::default();
        let in_flight = Arc::new(Semaphore::new(limits.in_flight));
        loop {
            let next = tokio::select! {
                n = reader.next() => n,
                // The writer gave up on the client: stop reading it too.
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
    if req.is_stream() {
        // An id used again for a new stream writes again.
        w.send(Out::Reopen(req.id)).await.map_err(|_| gone())?;
        let (tx, rx) = watch::channel(false);
        lock(streams).insert(req.id, tx);
        let streams = Arc::clone(streams);
        tokio::spawn(async move {
            topics::run(&d, &w, &req, rx).await;
            lock(&streams).remove(&req.id);
            drop(permit);
        });
    } else {
        tokio::spawn(async move {
            let response = unary::answer(&d, &req).await;
            let _ = send(&w, &reply(req.id, response)).await;
            drop(permit);
        });
    }
    Ok(())
}

pub(crate) fn reply(id: u64, response: Response) -> Frame {
    Frame {
        id,
        response: Some(response),
        ..Frame::default()
    }
}
