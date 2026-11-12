//! Moving frames: the writer task (batches frames, exports handles) and the reader task (decodes and dispatches).

use super::serve::Cancel;
use super::{Handler, Inner, Notices, Out, Peer, Pending, Side};
use crate::frame::{self, Decoder, Frame, Kind};
use crate::msg;
use crate::msg::error::{Error, ErrorKind};
use doze_os::channel::{self, Reader, Writer};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64};
use std::sync::{Arc, Mutex};
use tokio::sync::{Notify, mpsc};

/// Starts a peer on a channel. Must be called within a tokio runtime.
pub(crate) fn start(
    ch: channel::Channel,
    side: Side,
    handler: Handler,
    notices: Option<Notices>,
) -> Peer {
    let (r, w) = ch.split();
    let (out, rx) = mpsc::unbounded_channel();
    let inner = Arc::new(Inner {
        side,
        out,
        pending: Mutex::new(HashMap::new()),
        serving: Mutex::new(HashMap::new()),
        credits: Mutex::new(HashMap::new()),
        next: AtomicU32::new(1),
        closed: AtomicBool::new(false),
        closed_signal: Notify::new(),
        minor: AtomicU16::new(crate::version::CURRENT.minor),
        caps: AtomicU64::new(0),
    });
    tokio::spawn(writer(w, rx));
    let i = inner.clone();
    tokio::spawn(async move {
        let why = match reader(r, &i, handler, notices).await {
            Ok(()) => "the other side closed the channel".to_string(),
            Err(e) => e.message,
        };
        i.shut(&why);
    });
    Peer { inner }
}

async fn writer(mut w: Writer, mut rx: mpsc::UnboundedReceiver<Out>) {
    let mut batch = Vec::with_capacity(64 << 10);
    let mut next = None;
    loop {
        let item = match next.take() {
            Some(o) => o,
            None => match rx.recv().await {
                Some(o) => o,
                None => return,
            },
        };
        let ok = match item {
            Out::Pass {
                id,
                method,
                body,
                handle,
            } => pass(&mut w, id, method, body, handle).await,
            Out::Frame(f) => {
                batch.clear();
                batch.extend_from_slice(&f);
                while batch.len() < (256 << 10) {
                    match rx.try_recv() {
                        Ok(Out::Frame(f)) => batch.extend_from_slice(&f),
                        Ok(p) => {
                            next = Some(p);
                            break;
                        }
                        Err(_) => break,
                    }
                }
                w.write_all(&batch).await.is_ok()
            }
        };
        if !ok {
            return;
        }
    }
}

async fn pass(
    w: &mut Writer,
    id: u32,
    method: u16,
    body: Vec<u8>,
    handle: channel::Passable,
) -> bool {
    let Ok(exported) = w.export(handle) else {
        return false;
    };
    let Ok(bytes) = frame::encode(id, Kind::Call, method, &body, Some(exported.trailer())) else {
        return false;
    };
    w.write_exported(&bytes, exported).await.is_ok()
}

async fn reader(
    mut r: Reader,
    inner: &Arc<Inner>,
    handler: Handler,
    notices: Option<Notices>,
) -> Result<(), Error> {
    let mut d = Decoder::new(channel::TRAILER_LEN);
    let mut chunk = vec![0u8; 64 << 10];
    loop {
        while let Some(f) = d
            .next_frame()
            .map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))?
        {
            dispatch(inner, &mut r, &handler, notices.as_ref(), f)?;
        }
        let n = r
            .read(&mut chunk)
            .await
            .map_err(|e| Error::new(ErrorKind::Unavailable, e.to_string()))?;
        if n == 0 {
            return Ok(());
        }
        d.push(chunk.get(..n).unwrap_or_default());
    }
}

fn dispatch(
    inner: &Arc<Inner>,
    r: &mut Reader,
    handler: &Handler,
    notices: Option<&Notices>,
    f: Frame,
) -> Result<(), Error> {
    match f.kind {
        Kind::Call => super::incoming::incoming(inner, r, handler, f),
        Kind::Cancel => {
            let c = inner.serving.lock().ok().and_then(|mut s| s.remove(&f.id));
            c.iter().for_each(Cancel::fire);
            Ok(())
        }
        Kind::Credit => {
            let n = f
                .body
                .get(..4)
                .and_then(|b| b.try_into().ok())
                .map(u32::from_le_bytes)
                .unwrap_or(0);
            if let Some(c) = inner
                .credits
                .lock()
                .ok()
                .and_then(|c| c.get(&f.id).cloned())
            {
                if let Ok(mut o) = c.0.lock() {
                    o.grant(n);
                }
                c.1.notify_waiters();
            }
            Ok(())
        }
        Kind::Reply | Kind::Error | Kind::End => answer(inner, f),
        Kind::Item => item(inner, f),
        Kind::Event | Kind::Log => {
            if let (Side::Host, Some(n)) = (inner.side, notices) {
                n(f.kind, f.body);
            }
            Ok(())
        }
    }
}

fn answer(inner: &Arc<Inner>, f: Frame) -> Result<(), Error> {
    let Some(p) = inner.pending.lock().ok().and_then(|mut p| p.remove(&f.id)) else {
        return Ok(());
    };
    let result = match f.kind {
        Kind::Error => Err(msg::decode::<Error>(&f.body)
            .unwrap_or_else(|e| Error::new(ErrorKind::Internal, e.to_string()))),
        _ => Ok(f.body),
    };
    match (p, f.kind) {
        (Pending::Unary(_, tx), _) => drop(tx.send(result)),
        // End drops the sender: the stream reads `None`.
        (Pending::Stream(..), Kind::End) => {}
        (Pending::Stream(_, tx, _), _) => drop(tx.send(result)),
    }
    Ok(())
}

fn item(inner: &Arc<Inner>, f: Frame) -> Result<(), Error> {
    let mut pending = inner
        .pending
        .lock()
        .map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))?;
    if let Some(Pending::Stream(_, tx, flow)) = pending.get_mut(&f.id) {
        if !flow.arrived() {
            return Err(Error::new(
                ErrorKind::Internal,
                "the other side sent stream items beyond its credit",
            ));
        }
        let _ = tx.send(Ok(f.body));
    }
    Ok(())
}

impl Inner {
    /// The consumer of stream `id` took an item; `queued` are still waiting. Sends credit when due.
    pub(crate) fn consumed(&self, id: u32, queued: usize) {
        let grant = self
            .pending
            .lock()
            .ok()
            .and_then(|mut p| match p.get_mut(&id) {
                Some(Pending::Stream(_, _, flow)) => {
                    flow.consumed(u32::try_from(queued).unwrap_or(u32::MAX))
                }
                _ => None,
            });
        if let Some(n) = grant
            && let Ok(b) = frame::encode(id, Kind::Credit, 0, &n.to_le_bytes(), None)
        {
            let _ = self.queue(Out::Frame(b));
        }
    }
}
