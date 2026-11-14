//! One client connection (Go's `Server.handleConn`): the hello, refused in Go's words on a mismatch, then requests
//! until the client goes. Each request runs on a thread of its own; `cancel` ends the stream it names.

use crate::{Daemon, lock, topics, unary};
use doze_control::codec::{self, CodecError, Decoder};
use doze_control::{Frame, Hello, Op, PROTO_VERSION, Request, Response};
use doze_os::local::LocalStream;
use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

/// The write half, shared by the threads answering the connection; one frame is one write.
pub(crate) type Writer = Arc<Mutex<LocalStream>>;

/// What ends each stream of this connection, by request id.
type Streams = Arc<Mutex<BTreeMap<u64, watch::Sender<bool>>>>;

pub(crate) fn send(w: &Writer, f: &Frame) -> io::Result<()> {
    let line = codec::encode(f).map_err(io::Error::other)?;
    let mut w = lock(w);
    w.write_all(&line)?;
    w.flush()
}

/// The next request; `None` once the client has gone.
fn next(r: &mut LocalStream, dec: &mut Decoder) -> Option<Result<Request, CodecError>> {
    let mut buf = [0u8; 8192];
    loop {
        match dec.next_frame::<Request>() {
            Ok(Some(req)) => return Some(Ok(req)),
            Ok(None) => {}
            Err(e) => return Some(Err(e)),
        }
        match r.read(&mut buf) {
            Ok(0) | Err(_) => return None,
            Ok(n) => dec.push(buf.get(..n).unwrap_or_default()),
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
fn greet(w: &Writer, reader: &mut LocalStream, dec: &mut Decoder) -> bool {
    let first = match next(reader, dec) {
        Some(Ok(r)) => r,
        Some(Err(e)) => {
            let _ = send(w, &refusal(format!("bad request: {e}")));
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
            send(w, &frame).is_ok()
        }
        other => {
            let _ = send(w, &refusal(mismatch(other.as_ref())));
            false
        }
    }
}

pub(crate) fn serve(d: &Daemon, _conn: usize, stream: LocalStream) {
    let Ok(mut reader) = stream.try_clone() else {
        return;
    };
    let w: Writer = Arc::new(Mutex::new(stream));
    let mut dec = Decoder::new();
    if !greet(&w, &mut reader, &mut dec) {
        return;
    }
    let streams: Streams = Arc::default();
    loop {
        match next(&mut reader, &mut dec) {
            Some(Ok(req)) => dispatch(d, &w, &streams, req),
            Some(Err(e)) => {
                let _ = send(&w, &refusal(format!("bad request: {e}")));
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

fn dispatch(d: &Daemon, w: &Writer, streams: &Streams, req: Request) {
    if req.known_op() == Some(Op::Cancel) {
        if let Some(end) = lock(streams).get(&req.id) {
            let _ = end.send(true);
        }
        return;
    }
    let (d, w) = (d.clone(), Arc::clone(w));
    let spawned = if req.is_stream() {
        let (tx, rx) = watch::channel(false);
        lock(streams).insert(req.id, tx);
        let streams = Arc::clone(streams);
        std::thread::Builder::new()
            .name("doze-stream".into())
            .spawn(move || {
                topics::run(&d, &w, &req, rx);
                lock(&streams).remove(&req.id);
            })
    } else {
        std::thread::Builder::new()
            .name("doze-request".into())
            .spawn(move || {
                let response = unary::answer(&d, &req);
                let _ = send(&w, &reply(req.id, response));
            })
    };
    if let Err(err) = spawned {
        tracing::warn!("control: a request could not be run: {err}");
    }
}

pub(crate) fn reply(id: u64, response: Response) -> Frame {
    Frame {
        id,
        response: Some(response),
        ..Frame::default()
    }
}
