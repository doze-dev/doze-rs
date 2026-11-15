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
mod meter;

pub use facts::ClientQuotas;

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use kafka_meta::{QuotaEntity, QuotaName, QuotaValues};

use meter::{Rate, TokenBucket, throttle_ms, token_throttle_ms};

/// A meter that has recorded nothing for this long is dropped (`INACTIVE_SENSOR_EXPIRATION_TIME_SECONDS`).
const IDLE_MS: i64 = 3_600_000;

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

/// A level of the precedence: the user and client id parts of an entity (Kafka's `KafkaQuotaEntity`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Level {
    user: Option<QuotaName>,
    client: Option<QuotaName>,
}

impl Level {
    fn is(&self, user: Option<Part<'_>>, client: Option<Part<'_>>) -> bool {
        Part::matches(self.user.as_ref(), user) && Part::matches(self.client.as_ref(), client)
    }
}

/// One part of a level being looked for: a name, or the default.
#[derive(Debug, Clone, Copy)]
enum Part<'a> {
    Default,
    Named(&'a str),
}

impl Part<'_> {
    fn matches(have: Option<&QuotaName>, want: Option<Part<'_>>) -> bool {
        match (have, want) {
            (None, None) | (Some(QuotaName::Default), Some(Part::Default)) => true,
            (Some(QuotaName::Named(a)), Some(Part::Named(b))) => a == b,
            _ => false,
        }
    }
}

const CLIENT_LEVEL: u8 = 1;
const USER_LEVEL: u8 = 2;
const USER_CLIENT_LEVEL: u8 = 4;

/// What one client has used of one quota.
#[derive(Debug, Clone, Default)]
struct Meter {
    rate: Rate,
    tokens: TokenBucket,
    last_record_ms: i64,
    /// Until when its connection was last muted for this quota, and for how long.
    throttled_until_ms: i64,
    throttled_for_ms: i64,
}

/// The limits set for one quota, by level.
#[derive(Debug, Default)]
struct Limits(Vec<(Level, f64)>);

impl Limits {
    fn find(&self, user: Option<Part<'_>>, client: Option<Part<'_>>) -> Option<f64> {
        (self.0.iter())
            .find(|(level, _)| level.is(user, client))
            .map(|(_, v)| *v)
    }

    fn has(&self, user: Option<Part<'_>>, client: Option<Part<'_>>) -> bool {
        self.find(user, client).is_some()
    }

    /// The limit for the meter with these tags: `DefaultQuotaCallback.quotaLimit`. Tags with both parts look at the levels
    /// with both parts; a user alone at the user levels; a client id alone at the client id levels.
    fn limit(&self, user: &str, client: &str) -> Option<f64> {
        let (u, c) = (Part::Named(user), Part::Named(client));
        let d = Part::Default;
        match (user.is_empty(), client.is_empty()) {
            (false, false) => self
                .find(Some(u), Some(c))
                .or_else(|| self.find(Some(u), Some(d)))
                .or_else(|| self.find(Some(d), Some(c)))
                .or_else(|| self.find(Some(d), Some(d))),
            (false, true) => self
                .find(Some(u), None)
                .or_else(|| self.find(Some(d), None)),
            (true, false) => self
                .find(None, Some(c))
                .or_else(|| self.find(None, Some(d))),
            (true, true) => None,
        }
    }
}

/// One quota: the limits set for it, and the meters of the clients it has seen.
#[derive(Debug, Default)]
struct Kind {
    limits: Limits,
    /// Which levels have a limit (Kafka's `quotaTypesEnabled`): with one level only, every client is metered at that level
    /// without a search.
    levels: u8,
    /// By the meter's tags: the user and client id it is kept for (either may be empty: shared by every client id of a
    /// user, or by every user of a client id).
    meters: HashMap<(String, String), Meter>,
}

impl Kind {
    fn enabled(&self) -> bool {
        !self.limits.0.is_empty()
    }

    /// The tags a client's usage is kept under: `DefaultQuotaCallback.quotaMetricTags`. The tags are those of the most
    /// specific level with a limit that matches the client, in Kafka's order: user and client id, user and default client
    /// id, user, default user and client id, default user and default client id, default user, client id, default client
    /// id. A level of user alone is kept under the user (every client id of it shares the meter), a level of client id
    /// alone under the client id; any level with both parts, under both.
    fn tags<'a>(&self, who: Client<'a>) -> (&'a str, &'a str) {
        let (u, c) = (Part::Named(who.user), Part::Named(who.id));
        match self.levels {
            0 | CLIENT_LEVEL => ("", who.id),
            USER_LEVEL => (who.user, ""),
            USER_CLIENT_LEVEL => (who.user, who.id),
            _ => {
                let both = (who.user, who.id);
                let user_only = (who.user, "");
                let search = [
                    (Some(u), Some(c), both),
                    (Some(u), Some(Part::Default), both),
                    (Some(u), None, user_only),
                    (Some(Part::Default), Some(c), both),
                    (Some(Part::Default), Some(Part::Default), both),
                    (Some(Part::Default), None, user_only),
                ];
                search
                    .into_iter()
                    .find(|(user, client, _)| self.limits.has(*user, *client))
                    .map_or(("", who.id), |(_, _, tags)| tags)
            }
        }
    }

    /// The meter for `who`, made if it is new (and meters idle for an hour dropped then), and its limit.
    fn meter(&mut self, who: Client<'_>, now: i64) -> (&mut Meter, Option<f64>) {
        let (user, client) = self.tags(who);
        let limit = self.limits.limit(user, client);
        let key = (user.to_owned(), client.to_owned());
        if !self.meters.contains_key(&key) {
            self.meters.retain(|_, m| now - m.last_record_ms < IDLE_MS);
        }
        let meter = self.meters.entry(key).or_insert_with(|| Meter {
            last_record_ms: now,
            ..Meter::default()
        });
        (meter, limit)
    }

    fn set_limits(&mut self, limits: Vec<(Level, f64)>) {
        self.levels = limits.iter().fold(0, |acc, (level, _)| {
            acc | match (&level.user, &level.client) {
                (Some(_), Some(_)) => USER_CLIENT_LEVEL,
                (Some(_), None) => USER_LEVEL,
                _ => CLIENT_LEVEL,
            }
        });
        self.limits = Limits(limits);
        if !self.enabled() {
            self.meters = HashMap::new();
        }
    }
}

/// Kafka's quota bound when none applies (`ClientQuotaManager.quotaLimit`: `Long.MAX_VALUE`).
#[expect(
    clippy::cast_precision_loss,
    reason = "Kafka's own conversion of Long.MAX_VALUE to a double"
)]
const UNBOUNDED: f64 = i64::MAX as f64;

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
        Mutations {
            quotas: enabled.then_some(self),
            who,
            strict,
            last_throttle_ms: 0,
            last_recorded_ms: 0,
        }
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

impl Mutations<'_> {
    /// A request no quota applies to: the console's, and every request when none is set.
    pub(crate) const fn unbounded() -> Mutations<'static> {
        Mutations {
            quotas: None,
            who: Client { user: "", id: "" },
            strict: false,
            last_throttle_ms: 0,
            last_recorded_ms: 0,
        }
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

#[cfg(test)]
mod tests;
