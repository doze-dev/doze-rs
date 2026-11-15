//! The controller mutation quota of one request (KIP-599).

use super::meter::token_throttle_ms;
use super::{Client, QuotaKind, Quotas, UNBOUNDED};

/// The controller mutation quota of one CreateTopics, CreatePartitions or DeleteTopics request
/// (`StrictControllerMutationQuota`, `PermissiveControllerMutationQuota`).
#[derive(Debug)]
pub(crate) struct Mutations<'a> {
    /// `None`: no client has a mutation quota, and every mutation is taken (`UNBOUNDED_CONTROLLER_MUTATION_QUOTA`).
    quotas: Option<&'a Quotas>,
    who: Client<'a>,
    strict: bool,
    last_throttle_ms: i64,
    last_recorded_ms: i64,
}

/// A mutation refused because the quota is exhausted (THROTTLING_QUOTA_EXCEEDED).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Exhausted;

impl<'a> Mutations<'a> {
    /// The mutations of `who`, held to `quotas` (`None`: to nothing).
    pub(super) const fn new(quotas: Option<&'a Quotas>, who: Client<'a>, strict: bool) -> Self {
        Self {
            quotas,
            who,
            strict,
            last_throttle_ms: 0,
            last_recorded_ms: 0,
        }
    }

    /// A request no quota applies to: the console's, and every request when none is set.
    pub(crate) const fn unbounded() -> Mutations<'static> {
        Mutations::new(None, Client { user: "", id: "" }, false)
    }

    /// Records `partitions` mutations at `now`. Strict: refused while the bucket is below zero (checked before the
    /// mutation is counted, so the one that empties it is taken); permissive: always taken, the client throttled.
    pub(crate) fn record(&mut self, partitions: i32, now: i64) -> Result<(), Exhausted> {
        let Some(quotas) = self.quotas else {
            return Ok(());
        };
        let mut state = quotas.lock();
        let Some(k) = state.kinds.get_mut(QuotaKind::Mutation.index()) else {
            return Ok(());
        };
        let (meter, limit) = k.meter(self.who, now);
        let bound = limit.unwrap_or(UNBOUNDED);
        let permits = f64::from(partitions);
        if self.strict {
            let tokens = meter.tokens.measure(bound, now);
            if tokens < 0.0 {
                self.note(tokens, bound, now);
                return Err(Exhausted);
            }
        }
        meter.rate.record(permits, now);
        meter.tokens.record(permits, bound, now);
        meter.last_record_ms = now;
        if !self.strict {
            let tokens = meter.tokens.measure(bound, now);
            if tokens < 0.0 {
                self.note(tokens, bound, now);
            }
        }
        Ok(())
    }

    fn note(&mut self, tokens: f64, bound: f64, now: i64) {
        self.last_throttle_ms = token_throttle_ms(tokens, bound);
        self.last_recorded_ms = now;
    }

    /// How long the client is to be throttled for its mutations, counted down from when it was worked out:
    /// `AbstractControllerMutationQuota.throttleTime`.
    pub(crate) fn throttle_time(&self, now: i64) -> i32 {
        if self.last_throttle_ms == 0 {
            return 0;
        }
        let left = (self.last_throttle_ms - (now - self.last_recorded_ms)).max(0);
        i32::try_from(left).unwrap_or(i32::MAX)
    }
}
