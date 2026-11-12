//! The connection's shared state: what is pending, what is being served, stream credit, and what happens when the
//! channel ends.

use super::{Inner, Out, Pending, serve};
use crate::method::Method;
use crate::msg::error::{Error, ErrorKind};
use std::sync::atomic::Ordering;

impl Inner {
    pub(crate) fn queue(&self, o: Out) -> Result<(), Error> {
        self.out
            .send(o)
            .map_err(|_| Error::new(ErrorKind::Unavailable, "the connection ended"))
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    pub(crate) fn finish_serving(&self, id: u32) {
        if let Ok(mut s) = self.serving.lock() {
            s.remove(&id);
        }
        if let Ok(mut c) = self.credits.lock() {
            c.remove(&id);
        }
    }

    /// Fails every pending call and cancels every served one: the connection is gone.
    pub(crate) fn shut(&self, why: &str) {
        self.closed.store(true, Ordering::SeqCst);
        let pending = self
            .pending
            .lock()
            .map(|mut p| std::mem::take(&mut *p))
            .unwrap_or_default();
        for (_, p) in pending {
            let e = Error::new(ErrorKind::Unavailable, why);
            match p {
                Pending::Unary(_, tx) => drop(tx.send(Err(e))),
                Pending::Stream(_, tx, _) => drop(tx.send(Err(e))),
            }
        }
        let serving = self
            .serving
            .lock()
            .map(|mut s| std::mem::take(&mut *s))
            .unwrap_or_default();
        serving.values().for_each(serve::Cancel::fire);
        if let Ok(c) = self.credits.lock() {
            c.values().for_each(|c| c.1.notify_waiters());
        }
        self.closed_signal.notify_waiters();
    }

    /// Whether `id` is an outgoing call of `method` still in flight (a callback's scope).
    pub(crate) fn in_flight(&self, id: u32, method: Method) -> bool {
        self.pending.lock().is_ok_and(|p| match p.get(&id) {
            Some(Pending::Unary(m, _)) | Some(Pending::Stream(m, _, _)) => *m == method,
            None => false,
        })
    }
}
