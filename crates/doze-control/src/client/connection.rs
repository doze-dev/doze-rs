//! One greeted connection to a daemon. A reader thread splits the frames by request id and hands each request's
//! frames to its own `async_channel`, so one connection carries any number of calls and streams at once (Go's
//! `control.Conn`). Writes are serialised by a lock; reads never block a writer.

use super::stream::{Pending, Stream};
use super::wait::block_on;
use crate::codec::{self, Decoder};
use crate::conn::{Conn, Dialer};
use crate::{Error, Frame, PROTO_VERSION, Request, Subscription};
use async_channel::{Receiver, Sender};
use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

/// How long the daemon has to answer the hello (Go's 5 s deadline).
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(5);

/// Frames queued for one request before the reader waits for its consumer (Go buffers 64).
const QUEUE: usize = 1024;

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// An open, greeted connection. Cheap to clone; the connection closes when the last clone, and every stream and
/// pending call on it, is dropped, or when [`Connection::close`] is called.
#[derive(Debug, Clone)]
pub struct Connection {
    handle: Arc<Handle>,
    timeout: Option<Duration>,
}

/// Hangs up when the last user of the connection is gone; the reader thread holds only [`Shared`].
#[derive(Debug)]
struct Handle {
    shared: Arc<Shared>,
}

impl Drop for Handle {
    fn drop(&mut self) {
        let _ = self.shared.hangup.shutdown();
    }
}

#[derive(Debug)]
pub(crate) struct Shared {
    writer: Mutex<Box<dyn Conn>>,
    hangup: Box<dyn Conn>,
    next_id: AtomicU64,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    waiting: HashMap<u64, Waiter>,
    /// Why the connection ended, once it has.
    closed: Option<String>,
}

#[derive(Debug)]
struct Waiter {
    tx: Sender<Frame>,
    /// Answered by one frame, after which the request is done.
    unary: bool,
}

/// Reads one frame, blocking.
fn read_frame(r: &mut dyn Conn, dec: &mut Decoder) -> io::Result<Frame> {
    let mut buf = [0u8; 16 * 1024];
    loop {
        match dec.next_frame::<Frame>() {
            Ok(Some(f)) => return Ok(f),
            Ok(None) => {}
            Err(e) => return Err(io::Error::new(io::ErrorKind::InvalidData, e)),
        }
        match r.read(&mut buf) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "the daemon hung up",
                ));
            }
            Ok(n) => dec.push(buf.get(..n).unwrap_or_default()),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

impl Connection {
    /// Dials, exchanges hellos (as client `name`), and starts the reader thread.
    pub(crate) fn open(
        dialer: &dyn Dialer,
        name: &str,
        timeout: Option<Duration>,
    ) -> Result<Connection, Error> {
        let mut conn = dialer.dial().map_err(Error::Connect)?;
        conn.set_read_timeout(Some(HELLO_TIMEOUT))?;
        conn.write_all(&codec::encode(&Request::hello(name))?)?;
        let mut dec = Decoder::new();
        let hello =
            read_frame(conn.as_mut(), &mut dec).map_err(|e| Error::NoHello(e.to_string()))?;
        if !hello.error.is_empty() {
            return Err(Error::Refused(hello.error));
        }
        if hello.hello.map(|h| h.proto) != Some(PROTO_VERSION) {
            return Err(Error::Protocol);
        }
        conn.set_read_timeout(None)?;
        let reader = conn.try_clone_conn()?;
        let shared = Arc::new(Shared {
            hangup: conn.try_clone_conn()?,
            writer: Mutex::new(conn),
            next_id: AtomicU64::new(0),
            state: Mutex::new(State::default()),
        });
        let theirs = Arc::clone(&shared);
        thread::Builder::new()
            .name("doze-control".into())
            .spawn(move || theirs.read_loop(reader, dec.into_rest()))?;
        Ok(Connection {
            handle: Arc::new(Handle { shared }),
            timeout,
        })
    }

    pub(crate) fn shared(&self) -> &Shared {
        &self.handle.shared
    }

    /// The timeout unary helpers use (`None`: wait for ever).
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Sends a unary request; its response arrives on the returned [`Pending`].
    pub fn send(&self, req: Request) -> Result<Pending, Error> {
        let op = req.op.clone();
        let (id, rx) = self.shared().start(req, true)?;
        Ok(Pending::new(id, op, rx, self.clone()))
    }

    /// Sends a unary request and waits for its response, at most `timeout`. Giving up does not stop the daemon
    /// doing what was asked.
    pub fn call_with(
        &self,
        req: Request,
        timeout: Option<Duration>,
    ) -> Result<crate::Response, Error> {
        self.send(req)?.wait(timeout)
    }

    /// [`Connection::call_with`] the connection's own timeout.
    pub fn call(&self, req: Request) -> Result<crate::Response, Error> {
        self.call_with(req, self.timeout)
    }

    /// Opens a stream (`subscribe`, `events`, `logs` with `follow`) and waits for the daemon to acknowledge it.
    pub fn stream(&self, req: Request) -> Result<Stream, Error> {
        let op = req.op.clone();
        let (id, rx) = self.shared().start(req, false)?;
        let deadline = self.timeout.map(|t| Instant::now() + t);
        let first = block_on(rx.recv(), deadline);
        let ack = match first {
            None => Err(Error::Timeout(self.timeout.unwrap_or_default())),
            Some(Err(_)) => Err(Error::Closed(self.shared().why())),
            Some(Ok(f)) => match f.response {
                None => Err(Error::NoAck(op)),
                Some(r) if !r.error.is_empty() => Err(Error::from_response(&r)),
                Some(_) => Ok(()),
            },
        };
        if let Err(e) = ack {
            self.shared().forget(id);
            return Err(e);
        }
        Ok(Stream::new(id, rx, self.clone()))
    }

    /// Opens a subscription; see [`Connection::stream`].
    pub fn subscribe(&self, sub: &Subscription) -> Result<Stream, Error> {
        self.stream(Request::subscribe(sub))
    }

    /// Tells the daemon to end the stream `id`. Its end frame still arrives.
    pub fn cancel(&self, id: u64) -> Result<(), Error> {
        self.shared().write(&codec::encode(&Request::cancel(id))?)
    }

    /// Hangs up. Every call and stream in flight ends.
    pub fn close(&self) {
        let _ = self.shared().hangup.shutdown();
    }

    /// Why the connection ended, if it has.
    pub fn closed(&self) -> Option<String> {
        lock(&self.shared().state).closed.clone()
    }
}

impl Shared {
    fn read_loop(&self, mut r: Box<dyn Conn>, mut dec: Decoder) {
        let why = loop {
            match read_frame(r.as_mut(), &mut dec) {
                Ok(f) => self.deliver(f),
                Err(e) => break e.to_string(),
            }
        };
        let mut st = lock(&self.state);
        st.closed = Some(why);
        // Dropping the senders closes every receiver: each waiting request learns the connection is gone.
        st.waiting.clear();
    }

    /// Hands a frame to the request it answers. A request nobody listens to any more is forgotten.
    fn deliver(&self, f: Frame) {
        let id = f.id;
        let tx = {
            let mut st = lock(&self.state);
            let Some(w) = st.waiting.get(&id) else { return };
            let tx = w.tx.clone();
            if w.unary || f.end {
                st.waiting.remove(&id);
            }
            tx
        };
        if tx.send_blocking(f).is_err() {
            self.forget(id);
        }
    }

    fn start(&self, mut req: Request, unary: bool) -> Result<(u64, Receiver<Frame>), Error> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = async_channel::bounded(QUEUE);
        {
            let mut st = lock(&self.state);
            if let Some(why) = &st.closed {
                return Err(Error::Closed(why.clone()));
            }
            st.waiting.insert(id, Waiter { tx, unary });
        }
        req.id = id;
        let sent = codec::encode(&req)
            .map_err(Error::from)
            .and_then(|line| self.write(&line));
        if let Err(e) = sent {
            self.forget(id);
            return Err(e);
        }
        Ok((id, rx))
    }

    fn write(&self, line: &[u8]) -> Result<(), Error> {
        let mut w = lock(&self.writer);
        w.write_all(line)?;
        w.flush()?;
        Ok(())
    }

    pub(crate) fn forget(&self, id: u64) {
        lock(&self.state).waiting.remove(&id);
    }

    pub(crate) fn why(&self) -> String {
        lock(&self.state)
            .closed
            .clone()
            .unwrap_or_else(|| "it was closed".to_owned())
    }
}
