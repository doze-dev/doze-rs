//! Waiting on a future from a plain thread, with a deadline: the client's blocking calls wait on the same
//! `async_channel` receivers an async caller awaits, so there is one delivery path, and no runtime is needed.

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};
use std::time::Instant;

/// Wakes the thread that is waiting.
struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// Runs `fut` on this thread until it is ready, or until `deadline` passes (`None`: no deadline).
pub(crate) fn block_on<F: Future>(fut: F, deadline: Option<Instant>) -> Option<F::Output> {
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut fut = pin!(fut);
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return Some(v);
        }
        match deadline {
            None => thread::park(),
            Some(at) => {
                let now = Instant::now();
                if now >= at {
                    return None;
                }
                thread::park_timeout(at - now);
            }
        }
    }
}
