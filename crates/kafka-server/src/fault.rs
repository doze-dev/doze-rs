//! Fault rules: bending requests at the protocol edge, on purpose, for a while (the console's Lab).
//!
//! The connection loop asks a [`FaultPlan`] about each request before the broker sees it; the answer is to let it through or to
//! act on it: delay the answer, answer with an error code instead of handing the request over, or close the connection. A fault is
//! made of the request and the answer alone. It never reaches storage, the group machines or the transaction coordinator: a
//! failed Produce is not appended, a dropped connection is a closed socket. The broker's data is as it would have been.
//!
//! The plan is optional, like the observer: with none installed ([`crate::Config::fault`] is `None`, which is `--lab=false`) a
//! request costs one `Option` test. With the table installed and no rule switched on, one relaxed load more.
//!
//! Requests that log a client in are never touched, and neither are the console's own (its calls do not pass through the
//! connection loop at all; a connection whose principal is the console's is left alone by every rule that does not name it).

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

mod matching;
mod rule;
mod synth;
mod table;
mod targets;

pub use matching::Ask;
pub use rule::{
    AddError, Effect, Ended, EndedWhy, FirstHit, LastHit, LONGEST_DELAY_MS, LONGEST_NAME,
    LONGEST_PATTERN, LONGEST_RULE, Matcher, MOST_CLIENTS, MOST_RULES, Pattern, Rule, SHORTEST_RULE, Share, Spec,
};
pub use synth::{FAILABLE, can_fail, fail_response};
pub use table::{Clock, FaultTable, MOST_ENDED, SystemClock};
pub use targets::Names;

#[cfg(test)]
mod testing;

/// What a plan decides for a request.
#[derive(Debug)]
pub enum Verdict {
    /// Serve it as usual.
    Pass,
    /// Act on it.
    Act(Act),
}

/// What is done to a request, and the rule that did it.
#[derive(Debug)]
pub struct Act {
    /// What happens.
    pub effect: Applied,
    /// What the recorder is told.
    pub note: Arc<Note>,
}

/// What happens to a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    /// The answer waits this long after the broker has made it.
    Delay(Duration),
    /// The answer is this error code, and the broker is not asked.
    Fail(i16),
    /// The connection is closed with no answer.
    Drop,
}

/// Which rule touched a request: carried to the observer, so the wire can say so.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Note {
    /// The rule's number.
    pub rule: u32,
    /// Its name at the time.
    pub name: Arc<str>,
}

/// Decides, for each request, whether anything is done to it. Called on the connection's task, one request at a time: the work
/// must be bounded and must not block.
pub trait FaultPlan: Send + Sync + fmt::Debug {
    /// What to do with this request.
    fn judge(&self, ask: &Ask<'_>) -> Verdict;
}
