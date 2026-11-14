//! How long each service took to become ready while a verb runs, from the daemon's journal (Go `watchReady`): the
//! verb's answer does not say, its `ready` event does. Best effort: a daemon that cannot be subscribed to yields no
//! durations, and a line without one is still right.

use crate::stack::Stack;
use doze_control::types::journal::kind;
use doze_control::{Connection, Stream, StreamEvent, Subscription, Topic};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How long to wait for a ready event after a verb answered: they travel separately.
const GRACE: Duration = Duration::from_millis(150);
/// A verb that answered faster than this started nothing, so there is no event to wait for.
const INSTANT: Duration = Duration::from_millis(5);

/// The journal, followed from now on, on its own connection: the reader of a connection hands each stream its frames
/// in turn, so a journal nobody drains while a long verb runs must not share the verb's connection.
pub(crate) struct Ready {
    _conn: Option<Connection>,
    stream: Option<Stream>,
    took: HashMap<String, Duration>,
}

impl Ready {
    pub(crate) fn watch(stack: &Stack) -> Ready {
        let sub = Subscription::to(&[Topic::Journal]);
        let conn = stack.read().ok().flatten();
        Ready {
            stream: conn.as_ref().and_then(|c| c.subscribe(&sub).ok()),
            _conn: conn,
            took: HashMap::new(),
        }
    }

    /// How long `name` took to become ready, or zero when the journal did not say. `verb_took` is how long the
    /// verb itself took to answer.
    pub(crate) fn of(&mut self, name: &str, verb_took: Duration) -> Duration {
        let wait = if verb_took < INSTANT {
            Duration::ZERO
        } else {
            GRACE
        };
        let deadline = Instant::now() + wait;
        loop {
            if let Some(d) = self.took.remove(name) {
                return d;
            }
            let Some(stream) = &self.stream else {
                return Duration::ZERO;
            };
            let left = deadline.saturating_duration_since(Instant::now());
            match stream.next_timeout(left.max(Duration::from_millis(1))) {
                Some(StreamEvent::Frame(f)) => {
                    if let Some(e) = f.event.filter(|e| e.kind == kind::READY) {
                        self.took.insert(e.instance, e.took.to_std());
                    }
                }
                Some(StreamEvent::End(_) | StreamEvent::Lost(_)) => self.stream = None,
                None if Instant::now() >= deadline => return Duration::ZERO,
                None => {}
            }
        }
    }

    pub(crate) fn stop(self) {
        if let Some(s) = self.stream {
            s.cancel();
        }
    }
}
