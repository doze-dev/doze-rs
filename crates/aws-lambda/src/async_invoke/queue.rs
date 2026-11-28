//! The queue asynchronous invocations wait in for their function (`async_invoke.rs` says how it is used).

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, PoisonError};

/// The most events waiting, of every function, and the most bytes of their payloads.
pub(crate) const QUEUED_EVENTS: usize = 10_000;
pub(crate) const QUEUED_BYTES: usize = 64 * 1024 * 1024;
/// How many of one function's events run at once.
pub(crate) const RUNNING_PER_FUNCTION: usize = 8;

/// Events waiting, per function.
#[derive(Debug)]
pub(crate) struct Queue<T> {
    state: Mutex<Waiting<T>>,
}

#[derive(Debug)]
struct Waiting<T> {
    by_function: HashMap<String, Line<T>>,
    events: usize,
    bytes: usize,
}

#[derive(Debug)]
struct Line<T> {
    waiting: VecDeque<(T, usize)>,
    running: usize,
}

impl<T> Default for Queue<T> {
    fn default() -> Self {
        Self {
            state: Mutex::new(Waiting {
                by_function: HashMap::new(),
                events: 0,
                bytes: 0,
            }),
        }
    }
}

/// What became of an event offered to the queue.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Admit<T> {
    /// Run it now: a runner's place was taken for it.
    Run(T),
    Queued,
    /// The queue is full.
    Dropped(T),
}

impl<T> Queue<T> {
    /// Offers `item` (of `bytes` payload) for `function`.
    pub(crate) fn admit(&self, function: &str, item: T, bytes: usize) -> Admit<T> {
        let mut guard = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let s = &mut *guard;
        let line = s
            .by_function
            .entry(function.to_owned())
            .or_insert_with(|| Line {
                waiting: VecDeque::new(),
                running: 0,
            });
        if line.running < RUNNING_PER_FUNCTION {
            line.running += 1;
            return Admit::Run(item);
        }
        if s.events >= QUEUED_EVENTS || s.bytes + bytes > QUEUED_BYTES {
            return Admit::Dropped(item);
        }
        line.waiting.push_back((item, bytes));
        s.events += 1;
        s.bytes += bytes;
        Admit::Queued
    }

    /// A runner of `function` is done with its event: the next one, or `None` and its place is given back.
    pub(crate) fn next(&self, function: &str) -> Option<T> {
        let mut guard = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let s = &mut *guard;
        let line = s.by_function.get_mut(function)?;
        match line.waiting.pop_front() {
            Some((item, bytes)) => {
                s.events -= 1;
                s.bytes -= bytes;
                Some(item)
            }
            None => {
                line.running = line.running.saturating_sub(1);
                if line.running == 0 {
                    s.by_function.remove(function);
                }
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_function_runs_eight_at_once_and_the_rest_wait_in_order_within_the_bounds() {
        let q = Queue::default();
        for i in 0..RUNNING_PER_FUNCTION {
            assert_eq!(q.admit("f", i, 1), Admit::Run(i));
        }
        assert_eq!(q.admit("f", 100, 1), Admit::Queued);
        assert_eq!(q.admit("f", 101, 1), Admit::Queued);
        // Another function is not held up by the first.
        assert_eq!(q.admit("g", 7, 1), Admit::Run(7));
        assert_eq!(q.next("f"), Some(100));
        assert_eq!(q.next("f"), Some(101));
        for _ in 0..RUNNING_PER_FUNCTION {
            assert_eq!(q.next("f"), None);
        }
        assert!(
            !q.state.lock().unwrap().by_function.contains_key("f"),
            "an idle function keeps no entry"
        );
        for i in 0..RUNNING_PER_FUNCTION {
            assert_eq!(q.admit("f", i, 1), Admit::Run(i));
        }
        assert_eq!(q.admit("f", 1, QUEUED_BYTES), Admit::Queued);
        assert_eq!(q.admit("f", 2, 1), Admit::Dropped(2), "bytes are bounded");
        assert_eq!(q.next("f"), Some(1));
        for i in 0..QUEUED_EVENTS {
            assert_eq!(q.admit("f", i, 0), Admit::Queued);
        }
        assert_eq!(q.admit("f", 0, 0), Admit::Dropped(0), "events are bounded");
    }
}
