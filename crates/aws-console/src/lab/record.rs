//! What a rule remembers of its life, and the record of one that is over. Little: the counts, the first call it fired on, when the
//! latest was, and eight access keys (the hits of the rest are counted).

use std::sync::Arc;

use super::rule::Spec;

/// The first call a rule fired on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct First {
    /// When, in milliseconds since the epoch.
    pub at_ms: i64,
    /// The access key id, clipped.
    pub caller: Box<str>,
    /// The service's SDK id.
    pub service: &'static str,
    /// The operation, clipped.
    pub operation: Box<str>,
    /// What the call named, clipped.
    pub resource: Option<Box<str>>,
    /// The client, from its `User-Agent`.
    pub sdk: Option<Box<str>>,
}

/// A rule in force, with what it has done so far.
#[derive(Debug, Clone)]
pub struct Rule {
    /// Its number: unique for the life of the table, never reused.
    pub id: u32,
    /// What it was asked to be.
    pub spec: Spec,
    /// The name it goes by (the spec's, or one made from what it does).
    pub name: Arc<str>,
    /// When it was made, in milliseconds since the epoch.
    pub created_ms: i64,
    /// When it ends, in milliseconds since the epoch.
    pub ends_ms: i64,
    /// Whether it fires (a rule switched off still runs out).
    pub enabled: bool,
    /// How many calls fitted it.
    pub matched: u64,
    /// How many it fired on and won.
    pub hits: u64,
    /// The first call it fired on.
    pub first: Option<First>,
    /// When the latest was.
    pub last_ms: Option<i64>,
    /// The access keys it fired on, in the order they came.
    pub callers: Vec<Box<str>>,
    /// Hits by keys past [`super::rule::MOST_KEPT_CALLERS`].
    pub more_callers: u32,
}

/// Why a rule is no longer in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndedWhy {
    /// Its time ran out.
    Expired,
    /// It had fired as often as it was allowed.
    Spent,
    /// It was removed.
    Cleared,
    /// A rule with the same matcher and the same kind of effect took its place.
    Replaced,
}

/// A rule that is over.
#[derive(Debug, Clone)]
pub struct Ended {
    /// The rule as it stood at the end.
    pub rule: Rule,
    /// When it ended, in milliseconds since the epoch (for an expiry, when it was due).
    pub at_ms: i64,
    /// Why.
    pub why: EndedWhy,
}

/// A rule found by its number, in force or over.
#[derive(Debug, Clone)]
pub struct Found {
    pub rule: Rule,
    /// When and why it ended; `None` while it is in force.
    pub over: Option<(i64, EndedWhy)>,
}
