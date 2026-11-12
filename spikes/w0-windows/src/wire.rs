//! Transport: one unix stream socket per module, frames multiplexed by id in both
//! directions, streams, cancellation, and SCM_RIGHTS fds attached to a frame.
//!
//! Frame = 12-byte header + body. Header: body length u32, id u32, kind u8,
//! flags u8 (bit 0: an fd rides with this frame), method u16. Little-endian.
//! Bodies are minicbor here (the candidate); the codec bench compares the others.

use anyhow::{Result, anyhow, bail};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use crate::os::{self, Passable};
use tokio::sync::{mpsc, oneshot};

pub const HEADER: usize = 12;
pub const MAX_BODY: usize = 16 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    Call = 1,
    Reply = 2,
    Error = 3,
    Item = 4,
    End = 5,
    Cancel = 6,
}

impl Kind {
    fn from_u8(b: u8) -> Result<Kind> {
        Ok(match b {
            1 => Kind::Call,
            2 => Kind::Reply,
            3 => Kind::Error,
            4 => Kind::Item,
            5 => Kind::End,
            6 => Kind::Cancel,
            _ => bail!("unknown frame kind {b}"),
        })
    }
}

pub struct Frame {
    pub id: u32,
    pub kind: Kind,
    pub method: u16,
    pub body: Vec<u8>,
    pub fd: Option<Passable>,
}

enum Out {
    /// A ready frame.
    Bytes(Vec<u8>),
    /// A call carrying a handle: the writer exports it (the trailer changes the length) and sends it alone.
    Pass { id: u32, method: u16, body: Vec<u8>, pass: Passable },
}

fn encode_frame(id: u32, kind: Kind, method: u16, has_fd: bool, body: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(HEADER + body.len());
    v.extend_from_slice(&(body.len() as u32).to_le_bytes());
    v.extend_from_slice(&id.to_le_bytes());
    v.push(kind as u8);
    v.push(u8::from(has_fd));
    v.extend_from_slice(&method.to_le_bytes());
    v.extend_from_slice(body);
    v
}

// ---- the peer: multiplexed calls in both directions -------------------------------

pub struct Incoming {
    pub method: u16,
    pub body: Vec<u8>,
    pub fd: Option<Passable>,
    pub reply: Responder,
    /// Fires when the caller cancels (or the connection drops).
    pub cancelled: oneshot::Receiver<()>,
}

#[derive(Clone)]
pub struct Responder {
    id: u32,
    tx: mpsc::UnboundedSender<Out>,
}

impl Responder {
    fn send(&self, kind: Kind, body: &[u8]) -> Result<()> {
        self.tx
            .send(Out::Bytes(encode_frame(self.id, kind, 0, false, body)))
            .map_err(|_| anyhow!("connection closed"))
    }
    pub fn ok<T: minicbor::Encode<()>>(&self, v: &T) -> Result<()> {
        self.send(Kind::Reply, &minicbor::to_vec(v)?)
    }
    pub fn err(&self, msg: &str) -> Result<()> {
        self.send(Kind::Error, msg.as_bytes())
    }
    pub fn item<T: minicbor::Encode<()>>(&self, v: &T) -> Result<()> {
        self.send(Kind::Item, &minicbor::to_vec(v)?)
    }
    pub fn end(&self) -> Result<()> {
        self.send(Kind::End, &[])
    }
}

pub type Handler = Arc<dyn Fn(Peer, Incoming) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

enum Pending {
    Unary(oneshot::Sender<Result<Vec<u8>>>),
    Stream(mpsc::UnboundedSender<Result<Vec<u8>>>),
}

struct State {
    pending: HashMap<u32, Pending>,
    serving: HashMap<u32, oneshot::Sender<()>>,
    closed: bool,
}

#[derive(Clone)]
pub struct Peer {
    tx: mpsc::UnboundedSender<Out>,
    st: Arc<Mutex<State>>,
    next: Arc<AtomicU32>,
    /// Signalled once when the connection ends (EOF or error).
    pub closed: Arc<tokio::sync::Notify>,
}

pub struct Stream {
    id: u32,
    rx: mpsc::UnboundedReceiver<Result<Vec<u8>>>,
    peer: Peer,
}

impl Stream {
    pub async fn next<T: for<'a> minicbor::Decode<'a, ()>>(&mut self) -> Option<Result<T>> {
        let r = self.rx.recv().await?;
        Some(r.and_then(|b| minicbor::decode(&b).map_err(Into::into)))
    }
    pub fn cancel(self) {
        let _ = self.peer.tx.send(Out::Bytes(encode_frame(self.id, Kind::Cancel, 0, false, &[])));
        if let Ok(mut st) = self.peer.st.lock() {
            st.pending.remove(&self.id);
        }
    }
}

impl Peer {
    /// Starts the reader and writer tasks on `sock`. `handler` serves calls from the other side.
    pub fn start(conn: os::Conn, handler: Handler, peer_pid: u32) -> Peer {
        let (rd, mut wr) = os::split(conn);
        set_peer_pid(&mut wr, peer_pid);
        let (tx, rx) = mpsc::unbounded_channel::<Out>();
        let peer = Peer {
            tx,
            st: Arc::new(Mutex::new(State { pending: HashMap::new(), serving: HashMap::new(), closed: false })),
            next: Arc::new(AtomicU32::new(1)),
            closed: Arc::new(tokio::sync::Notify::new()),
        };
        tokio::spawn(writer(wr, rx));
        let p = peer.clone();
        tokio::spawn(async move {
            let err = reader(rd, p.clone(), handler).await;
            p.shutdown(err.err().map(|e| e.to_string()).unwrap_or_else(|| "connection closed".into()));
        });
        peer
    }

    fn shutdown(&self, why: String) {
        let (pending, serving) = match self.st.lock() {
            Ok(mut st) => {
                st.closed = true;
                (std::mem::take(&mut st.pending), std::mem::take(&mut st.serving))
            }
            Err(_) => return,
        };
        for (_, p) in pending {
            match p {
                Pending::Unary(tx) => {
                    let _ = tx.send(Err(anyhow!("{why}")));
                }
                Pending::Stream(tx) => {
                    let _ = tx.send(Err(anyhow!("{why}")));
                }
            }
        }
        for (_, c) in serving {
            let _ = c.send(());
        }
        self.closed.notify_waiters();
        self.closed.notify_one();
    }

    fn register(&self, p: Pending) -> Result<u32> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let mut st = self.st.lock().map_err(|_| anyhow!("poisoned"))?;
        if st.closed {
            bail!("connection closed");
        }
        st.pending.insert(id, p);
        Ok(id)
    }

    pub async fn call_raw(&self, method: u16, body: Vec<u8>, fd: Option<Passable>) -> Result<Vec<u8>> {
        let (tx, rx) = oneshot::channel();
        let id = self.register(Pending::Unary(tx))?;
        let out = match fd {
            Some(pass) => Out::Pass { id, method, body, pass },
            None => Out::Bytes(encode_frame(id, Kind::Call, method, false, &body)),
        };
        self.tx
            .send(out)
            .map_err(|_| anyhow!("connection closed"))?;
        rx.await.map_err(|_| anyhow!("connection closed"))?
    }

    pub async fn call<Q, R>(&self, method: u16, req: &Q) -> Result<R>
    where
        Q: minicbor::Encode<()>,
        R: for<'a> minicbor::Decode<'a, ()>,
    {
        let b = self.call_raw(method, minicbor::to_vec(req)?, None).await?;
        Ok(minicbor::decode(&b)?)
    }

    pub async fn call_fd<Q, R>(&self, method: u16, req: &Q, fd: Passable) -> Result<R>
    where
        Q: minicbor::Encode<()>,
        R: for<'a> minicbor::Decode<'a, ()>,
    {
        let b = self.call_raw(method, minicbor::to_vec(req)?, Some(fd)).await?;
        Ok(minicbor::decode(&b)?)
    }

    pub fn stream<Q: minicbor::Encode<()>>(&self, method: u16, req: &Q) -> Result<Stream> {
        let (tx, rx) = mpsc::unbounded_channel();
        let id = self.register(Pending::Stream(tx))?;
        self.tx
            .send(Out::Bytes(encode_frame(id, Kind::Call, method, false, &minicbor::to_vec(req)?)))
            .map_err(|_| anyhow!("connection closed"))?;
        Ok(Stream { id, rx, peer: self.clone() })
    }
}

#[cfg(windows)]
fn set_peer_pid(w: &mut os::Writer, pid: u32) {
    w.peer_pid = pid;
}
#[cfg(unix)]
fn set_peer_pid(_w: &mut os::Writer, _pid: u32) {}

async fn writer(mut wr: os::Writer, mut rx: mpsc::UnboundedReceiver<Out>) {
    let mut batch: Vec<u8> = Vec::with_capacity(64 << 10);
    while let Some(first) = rx.recv().await {
        let mut next = Some(first);
        while let Some(o) = next.take() {
            match o {
                // A frame with a handle goes alone, so (on Unix) the fd rides with its first byte.
                Out::Pass { id, method, mut body, pass } => {
                    let (trailer, ride) = match wr.export(pass) {
                        Ok(x) => x,
                        Err(e) => {
                            eprintln!("export failed: {e:#}");
                            return;
                        }
                    };
                    body.extend_from_slice(&trailer);
                    let bytes = encode_frame(id, Kind::Call, method, true, &body);
                    if wr.write(&bytes, ride).await.is_err() {
                        return;
                    }
                }
                Out::Bytes(b) => {
                    batch.clear();
                    batch.extend_from_slice(&b);
                    while batch.len() < (256 << 10) {
                        match rx.try_recv() {
                            Ok(Out::Bytes(b)) => batch.extend_from_slice(&b),
                            Ok(p) => {
                                next = Some(p);
                                break;
                            }
                            Err(_) => break,
                        }
                    }
                    if wr.write(&batch, None).await.is_err() {
                        return;
                    }
                }
            }
        }
    }
}

async fn reader(mut rd: os::Reader, peer: Peer, handler: Handler) -> Result<()> {
    let mut buf: Vec<u8> = Vec::with_capacity(64 << 10);
    let mut chunk = vec![0u8; 64 << 10];
    loop {
        let mut used = 0;
        while buf.len() - used >= HEADER {
            let h = &buf[used..used + HEADER];
            let len = u32::from_le_bytes([h[0], h[1], h[2], h[3]]) as usize;
            if len > MAX_BODY {
                bail!("frame of {len} bytes exceeds the limit");
            }
            if buf.len() - used < HEADER + len {
                break;
            }
            let id = u32::from_le_bytes([h[4], h[5], h[6], h[7]]);
            let kind = Kind::from_u8(h[8])?;
            let has_fd = h[9] & 1 == 1;
            let method = u16::from_le_bytes([h[10], h[11]]);
            let mut body = buf[used + HEADER..used + HEADER + len].to_vec();
            used += HEADER + len;
            let fd = if has_fd {
                if body.len() < os::TRAILER {
                    bail!("handle frame shorter than its trailer");
                }
                let trailer = body.split_off(body.len() - os::TRAILER);
                Some(rd.import(&trailer)?)
            } else {
                None
            };
            dispatch(&peer, &handler, Frame { id, kind, method, body, fd })?;
        }
        buf.drain(..used);
        let n = rd.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

fn dispatch(peer: &Peer, handler: &Handler, f: Frame) -> Result<()> {
    let mut st = peer.st.lock().map_err(|_| anyhow!("poisoned"))?;
    match f.kind {
        Kind::Call => {
            let (ctx, crx) = oneshot::channel();
            st.serving.insert(f.id, ctx);
            drop(st);
            let inc = Incoming {
                method: f.method,
                body: f.body,
                fd: f.fd,
                reply: Responder { id: f.id, tx: peer.tx.clone() },
                cancelled: crx,
            };
            let fut = handler(peer.clone(), inc);
            let p = peer.clone();
            let id = f.id;
            tokio::spawn(async move {
                fut.await;
                if let Ok(mut st) = p.st.lock() {
                    st.serving.remove(&id);
                }
            });
        }
        Kind::Cancel => {
            if let Some(c) = st.serving.remove(&f.id) {
                let _ = c.send(());
            }
        }
        Kind::Reply | Kind::Error => {
            let r = if f.kind == Kind::Reply { Ok(f.body) } else { Err(anyhow!("{}", String::from_utf8_lossy(&f.body))) };
            match st.pending.remove(&f.id) {
                Some(Pending::Unary(tx)) => {
                    let _ = tx.send(r);
                }
                Some(Pending::Stream(tx)) => {
                    let _ = tx.send(r);
                }
                None => {}
            }
        }
        Kind::Item => {
            if let Some(Pending::Stream(tx)) = st.pending.get(&f.id) {
                let _ = tx.send(Ok(f.body));
            }
        }
        Kind::End => {
            st.pending.remove(&f.id);
        }
    }
    Ok(())
}
