//! One quota's limits by level, the meters of its clients, and Kafka's rules for which limit and which meter a client gets.

use std::collections::HashMap;

use kafka_meta::QuotaName;

use super::meter::{Rate, TokenBucket};
use super::{Client, IDLE_MS};

/// A level of the precedence: the user and client id parts of an entity (Kafka's `KafkaQuotaEntity`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Level {
    pub(super) user: Option<QuotaName>,
    pub(super) client: Option<QuotaName>,
}

impl Level {
    pub(super) fn is(&self, user: Option<Part<'_>>, client: Option<Part<'_>>) -> bool {
        Part::matches(self.user.as_ref(), user) && Part::matches(self.client.as_ref(), client)
    }
}

/// One part of a level being looked for: a name, or the default.
#[derive(Debug, Clone, Copy)]
pub(super) enum Part<'a> {
    Default,
    Named(&'a str),
}

impl Part<'_> {
    pub(super) fn matches(have: Option<&QuotaName>, want: Option<Part<'_>>) -> bool {
        match (have, want) {
            (None, None) | (Some(QuotaName::Default), Some(Part::Default)) => true,
            (Some(QuotaName::Named(a)), Some(Part::Named(b))) => a == b,
            _ => false,
        }
    }
}

pub(super) const CLIENT_LEVEL: u8 = 1;
pub(super) const USER_LEVEL: u8 = 2;
pub(super) const USER_CLIENT_LEVEL: u8 = 4;

/// What one client has used of one quota.
#[derive(Debug, Clone, Default)]
pub(super) struct Meter {
    pub(super) rate: Rate,
    pub(super) tokens: TokenBucket,
    pub(super) last_record_ms: i64,
    /// Until when its connection was last muted for this quota, and for how long.
    pub(super) throttled_until_ms: i64,
    pub(super) throttled_for_ms: i64,
}

/// The limits set for one quota, by level.
#[derive(Debug, Default)]
pub(super) struct Limits(pub(super) Vec<(Level, f64)>);

impl Limits {
    pub(super) fn find(&self, user: Option<Part<'_>>, client: Option<Part<'_>>) -> Option<f64> {
        (self.0.iter())
            .find(|(level, _)| level.is(user, client))
            .map(|(_, v)| *v)
    }

    pub(super) fn has(&self, user: Option<Part<'_>>, client: Option<Part<'_>>) -> bool {
        self.find(user, client).is_some()
    }

    /// The limit for the meter with these tags: `DefaultQuotaCallback.quotaLimit`. Tags with both parts look at the levels
    /// with both parts; a user alone at the user levels; a client id alone at the client id levels.
    pub(super) fn limit(&self, user: &str, client: &str) -> Option<f64> {
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
pub(super) struct Kind {
    pub(super) limits: Limits,
    /// Which levels have a limit (Kafka's `quotaTypesEnabled`): with one level only, every client is metered at that level
    /// without a search.
    pub(super) levels: u8,
    /// By the meter's tags: the user and client id it is kept for (either may be empty: shared by every client id of a
    /// user, or by every user of a client id).
    pub(super) meters: HashMap<(String, String), Meter>,
}

impl Kind {
    pub(super) fn enabled(&self) -> bool {
        !self.limits.0.is_empty()
    }

    /// The tags a client's usage is kept under: `DefaultQuotaCallback.quotaMetricTags`. The tags are those of the most
    /// specific level with a limit that matches the client, in Kafka's order: user and client id, user and default client
    /// id, user, default user and client id, default user and default client id, default user, client id, default client
    /// id. A level of user alone is kept under the user (every client id of it shares the meter), a level of client id
    /// alone under the client id; any level with both parts, under both.
    pub(super) fn tags<'a>(&self, who: Client<'a>) -> (&'a str, &'a str) {
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
    pub(super) fn meter(&mut self, who: Client<'_>, now: i64) -> (&mut Meter, Option<f64>) {
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

    pub(super) fn set_limits(&mut self, limits: Vec<(Level, f64)>) {
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
