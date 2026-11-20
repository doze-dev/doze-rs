//! The flows of the map: one per caller and service, counted as each call is recorded (under the ring's own lock, on the
//! request path: no timer), so the map has totals since the start and not only what the ring still holds.
//!
//! A caller is the access key a request from the wire was signed with, or, for a call between services, the service that made
//! it. At most [`FLOWS`] flows are kept; a new one past that lets the one quiet the longest go (counted in `dropped`). A flow
//! costs its caller's text once, when it is first seen; counting a call into one that exists allocates nothing.

use super::Call;
use crate::model::Outcome;

/// The most flows kept.
pub const FLOWS: usize = 64;

/// The calls of one caller to one service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flow {
    /// The access key, or for a call between services the service that made it (`sns`); empty for a request that carried no key.
    pub caller: Box<str>,
    /// A call between services ([`Call::via`]).
    pub internal: bool,
    /// The service's name in a credential scope; empty for a request that named no service of this build.
    pub signing_name: &'static str,
    pub sdk_id: &'static str,
    pub calls: u64,
    /// Calls by outcome: ok, error, denied, not built.
    pub outcomes: [u64; 4],
    pub would_be_denied: u64,
    pub first_ms: u64,
    pub last_ms: u64,
    /// The number of its newest call.
    pub last_seq: u64,
}

#[derive(Debug, Default)]
pub(super) struct Flows {
    flows: Vec<Flow>,
    dropped: u64,
}

/// Who a call is from, as a flow names it.
fn caller_of(call: &Call) -> (&str, bool) {
    match &call.via {
        Some(via) if !via.by.is_empty() => (&via.by, true),
        Some(_) => (&call.caller, true),
        None => (&call.caller, false),
    }
}

const fn slot(outcome: Outcome) -> usize {
    match outcome {
        Outcome::Ok => 0,
        Outcome::Error => 1,
        Outcome::Denied => 2,
        Outcome::NotBuilt => 3,
    }
}

impl Flows {
    /// Counts `call` (its number set) into its flow.
    pub(super) fn count(&mut self, call: &Call) {
        let (caller, internal) = caller_of(call);
        let found = self.flows.iter().position(|f| {
            f.internal == internal
                && f.sdk_id == call.sdk_id
                && f.signing_name == call.signing_name
                && *f.caller == *caller
        });
        let at = if let Some(at) = found {
            at
        } else {
            if self.flows.len() >= FLOWS
                && let Some((quiet, _)) = self
                    .flows
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, f)| f.last_seq)
            {
                self.flows.swap_remove(quiet);
                self.dropped += 1;
            }
            self.flows.push(Flow {
                caller: caller.into(),
                internal,
                signing_name: call.signing_name,
                sdk_id: call.sdk_id,
                calls: 0,
                outcomes: [0; 4],
                would_be_denied: 0,
                first_ms: call.at_ms,
                last_ms: call.at_ms,
                last_seq: call.seq,
            });
            self.flows.len() - 1
        };
        let Some(flow) = self.flows.get_mut(at) else {
            return;
        };
        flow.calls += 1;
        if let Some(n) = flow
            .outcomes
            .get_mut(slot(Outcome::of(call.status, &call.code)))
        {
            *n += 1;
        }
        flow.would_be_denied += u64::from(call.would_be_denied);
        flow.last_ms = call.at_ms;
        flow.last_seq = call.seq;
    }

    /// Every flow, the most recently called first, and how many were let go.
    pub(super) fn all(&self) -> (Vec<Flow>, u64) {
        let mut all = self.flows.clone();
        all.sort_by_key(|f| std::cmp::Reverse(f.last_seq));
        (all, self.dropped)
    }
}
