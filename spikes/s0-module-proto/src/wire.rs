//! Transport: one unix stream socket per module, frames multiplexed by id in both
//! directions, streams, cancellation, and SCM_RIGHTS fds attached to a frame.
//!
//! Frame = 12-byte header + body. Header: body length u32, id u32, kind u8,
//! flags u8 (bit 0: an fd rides with this frame), method u16. Little-endian.
//! Bodies are minicbor here (the candidate); the codec bench compares the others.

use anyhow::{Context, Result, anyhow, bail};
use rustix::net::{
    RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer, SendAncillaryMessage, SendFlags,
};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::io::{IoSlice, IoSliceMut};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, OwnedFd};
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::Interest;
use tokio::net::UnixStream;
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
    pub fd: Option<OwnedFd>,
}

struct Out {
    bytes: Vec<u8>,
    fd: Option<OwnedFd>,
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

// ---- raw socket I/O with ancillary data ------------------------------------------

async fn send_all(sock: &UnixStream, bytes: &[u8], fd: Option<OwnedFd>) -> Result<()> {
    let mut off = 0;
    let mut fd = fd;
    while off < bytes.len() {
        let n = sock
            .async_io(Interest::WRITABLE, || {
                let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
                let mut anc = SendAncillaryBuffer::new(&mut space);
                let borrowed;
                if let Some(f) = fd.as_ref() {
                    borrowed = [f.as_fd()];
                    if !anc.push(SendAncillaryMessage::ScmRights(&borrowed)) {
                        return Err(std::io::Error::other("ancillary buffer too small"));
                    }
                }
                rustix::net::sendmsg(sock, &[IoSlice::new(&bytes[off..])], &mut anc, send_flags()).map_err(Into::into)
            })
            .await?;
        // The fd went with the first byte that was accepted; drop our copy.
        fd = None;
        off += n;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn send_flags() -> SendFlags {
    SendFlags::NOSIGNAL
}
#[cfg(not(target_os = "linux"))]
fn send_flags() -> SendFlags {
    SendFlags::empty()
}

/// Reads into `buf`, queueing any received fds. Returns 0 on EOF.
async fn recv_some(sock: &UnixStream, buf: &mut [u8], fds: &mut VecDeque<OwnedFd>) -> Result<usize> {
    let mut got: Vec<OwnedFd> = Vec::new();
    let n = sock
        .async_io(Interest::READABLE, || {
            let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(4))];
            let mut anc = RecvAncillaryBuffer::new(&mut space);
            let r = rustix::net::recvmsg(sock, &mut [IoSliceMut::new(buf)], &mut anc, recv_flags())?;
            for msg in anc.drain() {
                if let RecvAncillaryMessage::ScmRights(it) = msg {
                    got.extend(it);
                }
            }
            Ok(r.bytes)
        })
        .await?;
    for fd in got {
        // macOS has no MSG_CMSG_CLOEXEC: set it by hand so modules' children never inherit client sockets.
        rustix::io::fcntl_setfd(&fd, rustix::io::FdFlags::CLOEXEC)?;
        fds.push_back(fd);
    }
    Ok(n)
}

#[cfg(target_os = "linux")]
fn recv_flags() -> RecvFlags {
    RecvFlags::CMSG_CLOEXEC
}
#[cfg(not(target_os = "linux"))]
fn recv_flags() -> RecvFlags {
    RecvFlags::empty()
}

// ---- the peer: multiplexed calls in both directions -------------------------------

pub struct Incoming {
    pub method: u16,
    pub body: Vec<u8>,
    pub fd: Option<OwnedFd>,
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
            .send(Out { bytes: encode_frame(self.id, kind, 0, false, body), fd: None })
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
        let _ = self.peer.tx.send(Out { bytes: encode_frame(self.id, Kind::Cancel, 0, false, &[]), fd: None });
        if let Ok(mut st) = self.peer.st.lock() {
            st.pending.remove(&self.id);
        }
    }
}

impl Peer {
    /// Starts the reader and writer tasks on `sock`. `handler` serves calls from the other side.
    pub fn start(sock: UnixStream, handler: Handler) -> Peer {
        let sock = Arc::new(sock);
        let (tx, rx) = mpsc::unbounded_channel::<Out>();
        let peer = Peer {
            tx,
            st: Arc::new(Mutex::new(State { pending: HashMap::new(), serving: HashMap::new(), closed: false })),
            next: Arc::new(AtomicU32::new(1)),
            closed: Arc::new(tokio::sync::Notify::new()),
        };
        tokio::spawn(writer(sock.clone(), rx));
        let p = peer.clone();
        tokio::spawn(async move {
            let err = reader(sock, p.clone(), handler).await;
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

    pub async fn call_raw(&self, method: u16, body: Vec<u8>, fd: Option<OwnedFd>) -> Result<Vec<u8>> {
        let (tx, rx) = oneshot::channel();
        let id = self.register(Pending::Unary(tx))?;
        let has_fd = fd.is_some();
        self.tx
            .send(Out { bytes: encode_frame(id, Kind::Call, method, has_fd, &body), fd })
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

    pub async fn call_fd<Q, R>(&self, method: u16, req: &Q, fd: OwnedFd) -> Result<R>
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
            .send(Out { bytes: encode_frame(id, Kind::Call, method, false, &minicbor::to_vec(req)?), fd: None })
            .map_err(|_| anyhow!("connection closed"))?;
        Ok(Stream { id, rx, peer: self.clone() })
    }
}

async fn writer(sock: Arc<UnixStream>, mut rx: mpsc::UnboundedReceiver<Out>) {
    let mut batch: Vec<u8> = Vec::with_capacity(64 << 10);
    while let Some(first) = rx.recv().await {
        // A frame with an fd goes alone, so the fd rides with that frame's first byte.
        if first.fd.is_some() {
            if send_all(&sock, &first.bytes, first.fd).await.is_err() {
                return;
            }
            continue;
        }
        batch.clear();
        batch.extend_from_slice(&first.bytes);
        let mut held: Option<Out> = None;
        while batch.len() < (256 << 10) {
            match rx.try_recv() {
                Ok(o) if o.fd.is_none() => batch.extend_from_slice(&o.bytes),
                Ok(o) => {
                    held = Some(o);
                    break;
                }
                Err(_) => break,
            }
        }
        if send_all(&sock, &batch, None).await.is_err() {
            return;
        }
        if let Some(o) = held {
            if send_all(&sock, &o.bytes, o.fd).await.is_err() {
                return;
            }
        }
    }
}

async fn reader(sock: Arc<UnixStream>, peer: Peer, handler: Handler) -> Result<()> {
    let mut buf: Vec<u8> = Vec::with_capacity(64 << 10);
    let mut chunk = vec![0u8; 64 << 10];
    let mut fds: VecDeque<OwnedFd> = VecDeque::new();
    loop {
        // Parse every complete frame in the buffer.
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
            let body = buf[used + HEADER..used + HEADER + len].to_vec();
            used += HEADER + len;
            let fd = if has_fd { Some(fds.pop_front().context("frame announced an fd that did not arrive")?) } else { None };
            dispatch(&peer, &handler, Frame { id, kind, method, body, fd })?;
        }
        buf.drain(..used);
        let n = recv_some(&sock, &mut chunk, &mut fds).await?;
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
