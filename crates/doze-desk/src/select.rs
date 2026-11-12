//! Waiting on channels from a plain thread: a `block_on` that parks the thread, and a wait on several receivers at
//! once. No reactor: the app has none (PLAN.md §4.3), and these channels are all a thread here waits on.

use async_channel::Receiver;
use std::future::{Future, poll_fn};
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};
use std::time::Instant;

struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// Runs `f` to its end on this thread, or until `deadline` (then `None`).
pub(crate) fn block_on<F: Future>(f: F, deadline: Option<Instant>) -> Option<F::Output> {
    let mut f = pin!(f);
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return Some(v);
        }
        match deadline {
            None => thread::park(),
            Some(d) => {
                let now = Instant::now();
                if now >= d {
                    return None;
                }
                thread::park_timeout(d.saturating_duration_since(now));
            }
        }
    }
}

/// What woke a waiter on a command channel and several event channels. `None`: that channel is closed.
#[derive(Debug)]
pub(crate) enum Woke<C, E> {
    Command(Option<C>),
    /// An event, from the receiver at this place in the list.
    Event(usize, Option<E>),
}

type Recv<'a, T> = Pin<Box<dyn Future<Output = Result<T, async_channel::RecvError>> + Send + 'a>>;

/// Waits until the commands or one of `events` has something. Commands are asked first; the events starting at
/// `turn`, so a busy one cannot starve the rest.
pub(crate) fn any<C: Send, E: Send>(
    commands: &Receiver<C>,
    events: &[&Receiver<E>],
    turn: usize,
) -> Woke<C, E> {
    let mut cmd = pin!(commands.recv());
    let mut futs: Vec<Recv<'_, E>> = events
        .iter()
        .map(|r| Box::pin(r.recv()) as Recv<'_, E>)
        .collect();
    let n = futs.len();
    let woke = block_on(
        poll_fn(|cx| {
            if let Poll::Ready(r) = cmd.as_mut().poll(cx) {
                return Poll::Ready(Woke::Command(r.ok()));
            }
            for i in (0..n).map(|k| (turn + k) % n.max(1)) {
                if let Some(f) = futs.get_mut(i)
                    && let Poll::Ready(r) = f.as_mut().poll(cx)
                {
                    return Poll::Ready(Woke::Event(i, r.ok()));
                }
            }
            Poll::Pending
        }),
        None,
    );
    woke.unwrap_or(Woke::Command(None))
}
