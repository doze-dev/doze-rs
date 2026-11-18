//! Client quotas: what is set (durably, in the metadata store) and what each client has used, measured and enforced as
//! Kafka's quota managers do (`server/.../quota/ClientQuotaManager.java`, `ClientRequestQuotaManager.java`,
//! `ControllerMutationQuotaManager.java`).
//!
//! Four quotas, each held to separately: the bytes a client produces per second (`producer_byte_rate`), the bytes it is
//! sent by Fetch per second (`consumer_byte_rate`), the share of a request thread its requests take (`request_percentage`,
//! a percentage of one thread over the window) and the partitions it creates or deletes per second
//! (`controller_mutation_rate`). Each is set for a user, a client id, a user and client id together, or the default of
//! either; the most specific that matches a client applies ([`Kind::tags`], [`Kind::limit`]).
//!
//! With no quota set, nothing here allocates or runs: a request pays one atomic load ([`Quotas::enabled`]).

pub(crate) mod entity;
mod facts;
mod kind;
mod meter;
mod mutations;

pub use facts::ClientQuotas;
pub(crate) use mutations::Mutations;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use kafka_meta::{QuotaEntity, QuotaValues};

use kind::{Kind, Level};
use meter::throttle_ms;

/// A meter that has recorded nothing for this long is dropped (`INACTIVE_SENSOR_EXPIRATION_TIME_SECONDS`).
pub(super) const IDLE_MS: i64 = 3_600_000;

/// One of the four quotas a user or client id can be held to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum QuotaKind {
    /// `producer_byte_rate`: Produce request bytes per second.
    Produce,
    /// `consumer_byte_rate`: Fetch response bytes per second.
    Fetch,
    /// `request_percentage`: the time a request thread spends on the client's requests, as a percentage of one thread.
    Request,
    /// `controller_mutation_rate`: partitions created or deleted per second.
    Mutation,
}

impl QuotaKind {
    /// The four, in the order Kafka lists their keys.
    pub const ALL: [Self; 4] = [Self::Produce, Self::Fetch, Self::Request, Self::Mutation];

    /// The quota's key, as AlterClientQuotas and `kafka-configs.sh` name it.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Produce => "producer_byte_rate",
            Self::Fetch => "consumer_byte_rate",
            Self::Request => "request_percentage",
            Self::Mutation => "controller_mutation_rate",
        }
    }

    /// The quota a key names, if it names one of the four.
    #[must_use]
    pub fn of_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }

    const fn index(self) -> usize {
        match self {
            Self::Produce => 0,
            Self::Fetch => 1,
            Self::Request => 2,
            Self::Mutation => 3,
        }
    }
}

/// Who a request is from, as quotas see it: the principal's name (`alice` of `User:alice`) and the client id of the
/// request's header (empty if it gave none).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Client<'a> {
    pub(crate) user: &'a str,
    pub(crate) id: &'a str,
}

/// Kafka's quota bound when none applies (`ClientQuotaManager.quotaLimit`: `Long.MAX_VALUE`).
#[expect(
    clippy::cast_precision_loss,
    reason = "Kafka's own conversion of Long.MAX_VALUE to a double"
)]
pub(super) const UNBOUNDED: f64 = i64::MAX as f64;

#[derive(Debug, Default)]
struct State {
    /// Every entity with a quota and its values, as Describe answers them (addresses included).
    set: BTreeMap<QuotaEntity, BTreeMap<String, f64>>,
    kinds: [Kind; 4],
}

/// The quotas of one broker.
#[derive(Debug, Default)]
pub(crate) struct Quotas {
    /// Whether any of the four quotas has a limit anywhere: the one load a request pays when none has.
    on: AtomicBool,
    state: Mutex<State>,
}

impl Quotas {
    /// The quotas as they were stored.
    pub(crate) fn load(stored: Vec<(QuotaEntity, QuotaValues)>) -> Self {
        let quotas = Self::default();
        quotas.replace(stored.into_iter().map(|(e, v)| (e, v.0)).collect());
        quotas
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether any client could be held to a quota: one atomic load.
    pub(crate) fn enabled(&self) -> bool {
        self.on.load(Ordering::Relaxed)
    }

    /// Every entity with a quota, and its values.
    pub(crate) fn set(&self) -> BTreeMap<QuotaEntity, BTreeMap<String, f64>> {
        self.lock().set.clone()
    }

    /// Puts `set` in force, keeping what each client has used so far (a limit that changes applies to the usage already
    /// measured, as Kafka's `updateQuotaMetricConfigs` does). Callers hold the broker's quota lock.
    pub(crate) fn replace(&self, set: BTreeMap<QuotaEntity, BTreeMap<String, f64>>) {
        let mut state = self.lock();
        let mut limits: [Vec<(Level, f64)>; 4] = Default::default();
        for (entity, values) in &set {
            if entity.ip.is_some() {
                continue;
            }
            for (key, value) in values {
                if let Some(kind) = QuotaKind::of_key(key)
                    && let Some(slot) = limits.get_mut(kind.index())
                {
                    let level = Level {
                        user: entity.user.clone(),
                        client: entity.client_id.clone(),
                    };
                    slot.push((level, *value));
                }
            }
        }
        for (kind, limits) in state.kinds.iter_mut().zip(limits) {
            kind.set_limits(limits);
        }
        let on = state.kinds.iter().any(Kind::enabled);
        state.set = set;
        drop(state);
        self.on.store(on, Ordering::Relaxed);
    }

    /// Records `value` against `who`'s meter of `kind` and says how long to throttle it, in milliseconds (0: it is within
    /// its quota): `ClientQuotaManager.maybeRecordAndGetThrottleTimeMs`. A kind with no limit anywhere records nothing.
    /// The value counts whatever the answer: a throttled client has still used what it used.
    pub(crate) fn record(&self, kind: QuotaKind, who: Client<'_>, value: f64, now: i64) -> i32 {
        let mut state = self.lock();
        let Some(k) = state.kinds.get_mut(kind.index()).filter(|k| k.enabled()) else {
            return 0;
        };
        let (meter, limit) = k.meter(who, now);
        meter.rate.record(value, now);
        meter.last_record_ms = now;
        let bound = limit.unwrap_or(UNBOUNDED);
        let measured = meter.rate.measure(now);
        if measured <= bound {
            return 0;
        }
        let wait = throttle_ms(measured, bound, meter.rate.window_size(now));
        // The request quota never throttles for longer than one window (`ClientRequestQuotaManager.throttleTime`).
        let wait = if kind == QuotaKind::Request {
            wait.min(meter::WINDOW_MS)
        } else {
            wait
        };
        i32::try_from(wait.max(0)).unwrap_or(i32::MAX)
    }

    /// Takes back a value recorded for a request that was then not served (a throttled Fetch is answered empty):
    /// `ClientQuotaManager.unrecordQuotaSensor`.
    pub(crate) fn unrecord(&self, kind: QuotaKind, who: Client<'_>, value: f64, now: i64) {
        let mut state = self.lock();
        if let Some(k) = state.kinds.get_mut(kind.index()).filter(|k| k.enabled()) {
            let (meter, _) = k.meter(who, now);
            meter.rate.record(-value, now);
            meter.last_record_ms = now;
        }
    }

    /// The most a client can be sent in one go without being throttled for it however idle it has been, if a limit
    /// applies: `ClientQuotaManager.maxValueInQuotaWindow` (the limit times the span less one window).
    pub(crate) fn most_in_window(&self, kind: QuotaKind, who: Client<'_>) -> Option<f64> {
        let state = self.lock();
        let k = state.kinds.get(kind.index()).filter(|k| k.enabled())?;
        let (user, client) = k.tags(who);
        #[expect(clippy::cast_precision_loss, reason = "10 windows of one second")]
        let span = ((meter::SAMPLES as i64 - 1) * meter::WINDOW_MS) as f64 / 1000.0;
        k.limits.limit(user, client).map(|limit| limit * span)
    }

    /// Notes that `who`'s connection is muted for `ms` because of `kind`, for the console to show.
    pub(crate) fn throttled(&self, kind: QuotaKind, who: Client<'_>, ms: i32, now: i64) {
        let mut state = self.lock();
        if let Some(k) = state.kinds.get_mut(kind.index()).filter(|k| k.enabled()) {
            let (meter, _) = k.meter(who, now);
            meter.throttled_until_ms = now + i64::from(ms);
            meter.throttled_for_ms = i64::from(ms);
        }
    }

    /// The controller mutation quota of one request: `ControllerMutationQuotaManager.newQuotaFor`. Strict (a request at
    /// or above the version that knows THROTTLING_QUOTA_EXCEEDED) refuses a mutation while the quota is exhausted;
    /// permissive takes it and throttles the client.
    pub(crate) fn mutations<'a>(&'a self, who: Client<'a>, strict: bool) -> Mutations<'a> {
        let enabled = self.enabled()
            && self
                .lock()
                .kinds
                .get(QuotaKind::Mutation.index())
                .is_some_and(Kind::enabled);
        Mutations::new(enabled.then_some(self), who, strict)
    }

    /// What each client has used of each quota, its limit, and whether it is muted now, for the console.
    pub(crate) fn usage(&self, now: i64) -> Vec<Usage> {
        let mut state = self.lock();
        let mut out = Vec::new();
        for kind in QuotaKind::ALL {
            let Some(Kind { limits, meters, .. }) = state.kinds.get_mut(kind.index()) else {
                continue;
            };
            for ((user, client), meter) in meters.iter_mut() {
                if now - meter.last_record_ms >= IDLE_MS {
                    continue;
                }
                out.push(Usage {
                    kind,
                    user: user.clone(),
                    client_id: client.clone(),
                    rate: meter.rate.measure(now),
                    limit: limits.limit(user, client),
                    throttled_until_ms: meter.throttled_until_ms,
                    throttled_for_ms: meter.throttled_for_ms,
                    last_record_ms: meter.last_record_ms,
                });
            }
        }
        out.sort_by(|a, b| (a.kind, &a.user, &a.client_id).cmp(&(b.kind, &b.user, &b.client_id)));
        out
    }
}

/// What one client has used of one quota lately.
#[derive(Debug, Clone, PartialEq)]
pub struct Usage {
    pub kind: QuotaKind,
    /// The user the usage is kept for; empty when it is kept for a client id across users.
    pub user: String,
    /// The client id the usage is kept for; empty when it is kept for a user across its client ids.
    pub client_id: String,
    /// Per second over the measured span (bytes, percent of a thread, or partitions).
    pub rate: f64,
    /// The limit that applies, if one does.
    pub limit: Option<f64>,
    /// Until when (milliseconds since the epoch) its connection was last muted for this quota, and for how long.
    pub throttled_until_ms: i64,
    pub throttled_for_ms: i64,
    pub last_record_ms: i64,
}

#[cfg(test)]
mod tests;
