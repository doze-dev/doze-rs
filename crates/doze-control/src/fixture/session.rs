//! One client connection to the fixture daemon, as Go's `Server.handleConn`: the hello (refused in words on a
//! mismatch), then requests until the client goes away. Unary requests are answered at once; each stream runs on its
//! own thread, so one connection carries several.

use super::{Msg, State, play, unary};
use crate::client::connection::lock;
use crate::codec::{self, CodecError, Decoder};
use crate::{Frame, Hello, Op, PROTO_VERSION, Request};
use doze_os::local::LocalStream;
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;

/// The write half of a connection, shared by the threads answering it.
pub(crate) type Writer = Arc<Mutex<LocalStream>>;

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
fn mismatch(hello: Option<&Hello>) -> String {
    let spoke = hello.map_or_else(
        || "no protocol version".to_owned(),
        |h| format!("protocol {}", h.proto),
    );
    format!(
        "this daemon speaks control protocol {PROTO_VERSION} and the client sent {spoke} — they are different versions of doze; run `doze down` and try again"
    )
}

/// An error frame: what Go's server sends a connection it refuses.
fn refusal(error: String) -> Frame {
    Frame {
        error,
        ..Frame::default()
    }
}

/// Reads the hello and answers it. `false` if the connection is refused or gone.
fn greet(state: &State, w: &Writer, reader: &mut LocalStream, dec: &mut Decoder) -> bool {
    let first = match next(reader, dec) {
        Some(Ok(r)) => r,
        Some(Err(e)) => {
            let _ = send(w, &refusal(format!("bad request: {e}")));
            return false;
        }
        None => return false,
    };
    lock(&state.requests).push(first.clone());
    match &first.hello {
        Some(h) if h.proto == PROTO_VERSION => {
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

pub(crate) fn serve(state: &Arc<State>, conn: usize, stream: LocalStream) {
    let Ok(mut reader) = stream.try_clone() else {
        return;
    };
    let w: Writer = Arc::new(Mutex::new(stream));
    let mut dec = Decoder::new();
    if !greet(state, &w, &mut reader, &mut dec) {
        return;
    }
    while let Some(Ok(req)) = next(&mut reader, &mut dec) {
        lock(&state.requests).push(req.clone());
        if req.known_op() == Some(Op::Cancel) {
            if let Some(live) = lock(&state.streams).get(&(conn, req.id)) {
                let _ = live.tx.try_send(Msg::End(None));
            }
        } else if req.is_stream() {
            let (state, w) = (Arc::clone(state), Arc::clone(&w));
            let _ = thread::Builder::new()
                .name("fixture-stream".into())
                .spawn(move || play::run(&state, conn, &w, &req));
        } else {
            let frame = Frame {
                id: req.id,
                response: Some(unary::answer(state, &req)),
                ..Frame::default()
            };
            if send(&w, &frame).is_err() {
                break;
            }
        }
    }
    // The client went away: a stream it started ends with it.
    for ((c, _), live) in lock(&state.streams).iter() {
        if *c == conn {
            let _ = live.tx.try_send(Msg::End(None));
        }
    }
}
