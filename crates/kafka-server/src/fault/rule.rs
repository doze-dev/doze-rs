//! What a fault rule is: who it matches, what it does, how often, and for how long. Plain values with no clock and no lock;
//! [`super::FaultTable`] holds them.

use std::sync::Arc;
use std::time::Duration;

use kafka_wire::messages::ApiKey;

/// The most rules at once.
pub const MOST_RULES: usize = 50;
/// The longest a rule may last.
pub const LONGEST_RULE: Duration = Duration::from_secs(24 * 3600);
/// The shortest a rule may last.
pub const SHORTEST_RULE: Duration = Duration::from_secs(1);
/// The longest delay a rule may add (and jitter).
pub const LONGEST_DELAY_MS: u32 = 60_000;
/// The longest a rule's name is, in characters.
pub const LONGEST_NAME: usize = 80;
/// The longest a pattern is, in bytes (a topic name is at most 249).
pub const LONGEST_PATTERN: usize = 249;
/// The most clients a rule remembers having touched; the rest are counted.
pub const MOST_CLIENTS: usize = 8;

/// A text that may hold `*` (any run of characters) and `?` (any one character). Without either it is an exact name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern(Box<str>);

impl Pattern {
    /// A pattern, or `None` if it is empty, too long or holds a control character.
    #[must_use]
    pub fn new(text: &str) -> Option<Self> {
        let ok = !text.is_empty()
            && text.len() <= LONGEST_PATTERN
            && !text.chars().any(char::is_control);
        ok.then(|| Self(text.into()))
    }

    /// The text as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether `name` fits. The walk is linear in the two lengths: a hostile pattern cannot make it slow.
    #[must_use]
    pub fn fits(&self, name: &str) -> bool {
        glob(self.0.as_bytes(), name.as_bytes())
    }
}

/// Greedy glob with one backtrack point (the last `*`): O(pattern x name) at worst, no recursion.
fn glob(p: &[u8], n: &[u8]) -> bool {
    let (mut pi, mut ni) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        match p.get(pi) {
            Some(b'*') => {
                star = Some((pi, ni));
                pi += 1;
            }
            Some(&c) if c == b'?' || Some(&c) == n.get(ni) => {
                pi += 1;
                ni += 1;
            }
            _ => match star {
                Some((sp, sn)) => {
                    pi = sp + 1;
                    ni = sn + 1;
                    star = Some((sp, sn + 1));
                }
                None => return false,
            },
        }
    }
    p.get(pi..).is_some_and(|rest| rest.iter().all(|&c| c == b'*'))
}

/// Which requests a rule touches. Every field that is set must fit; a field left empty fits anything.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Matcher {
    /// The client's `client.id`.
    pub client: Option<Pattern>,
    /// The principal, written `Type:name`, exactly.
    pub principal: Option<Box<str>>,
    /// The APIs; empty is any (a rule that fails requests must name them).
    pub apis: Vec<ApiKey>,
    /// The topics a request names; set, a request that names none does not fit.
    pub topics: Vec<Pattern>,
    /// The partitions of those topics; empty is any.
    pub partitions: Vec<i32>,
    /// The group of a group API.
    pub group: Option<Pattern>,
    /// Only connections numbered up to this one (the rule closes what was open when it was made, not what comes after).
    pub up_to_conn: Option<u64>,
}

/// What a rule does to a request it fires on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Answer after the normal time plus `delay_ms` and a random 0..=`jitter_ms`.
    Slow { delay_ms: u32, jitter_ms: u32 },
    /// Answer with this error code in the shape of the API, without handing the request to the broker.
    Fail { code: i16 },
    /// Close the connection without answering.
    Drop,
}

impl Effect {
    /// A short word for it: `slow`, `fail`, `drop`.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Slow { .. } => "slow",
            Self::Fail { .. } => "fail",
            Self::Drop => "drop",
        }
    }
}

/// How many of the matching requests a rule fires on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Share {
    /// Every one.
    Every,
    /// Every Nth (2 to 1,000): the Nth, the 2Nth...
    EveryNth(u32),
    /// A chance of this many in a hundred (1 to 99), drawn at random.
    Percent(u8),
}

/// A rule as it is asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    /// What the rule is called; empty is named after its effect.
    pub name: String,
    /// Who it touches.
    pub matcher: Matcher,
    /// What it does.
    pub effect: Effect,
    /// How often.
    pub share: Share,
    /// How long it lasts, 1 s to 24 h.
    pub ttl: Duration,
    /// It ends after firing this many times, if that comes first.
    pub max_hits: Option<u32>,
}

/// Why a rule was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddError {
    /// There are already [`MOST_RULES`].
    TooMany,
    /// The name is longer than [`LONGEST_NAME`] or holds a control character.
    BadName,
    /// The duration is outside [`SHORTEST_RULE`]..=[`LONGEST_RULE`].
    BadDuration,
    /// A delay or jitter above [`LONGEST_DELAY_MS`], or a delay of nothing.
    BadDelay,
    /// A share outside its range.
    BadShare,
    /// An error code Kafka has no name for, or 0.
    BadCode,
    /// A rule that fails requests names the APIs it fails.
    FailNeedsApi,
    /// The API has no answer the lab can make.
    CannotFail(ApiKey),
    /// A partition below zero, or more than 64 of them.
    BadPartition,
    /// More than 16 topics.
    TooManyTopics,
    /// `max_hits` of 0.
    BadHits,
}

impl Spec {
    /// Whether the rule may be made.
    ///
    /// # Errors
    /// The first thing wrong with it.
    pub fn check(&self) -> Result<(), AddError> {
        if self.name.chars().count() > LONGEST_NAME || self.name.chars().any(char::is_control) {
            return Err(AddError::BadName);
        }
        if !(SHORTEST_RULE..=LONGEST_RULE).contains(&self.ttl) {
            return Err(AddError::BadDuration);
        }
        if self.max_hits == Some(0) {
            return Err(AddError::BadHits);
        }
        self.check_share()?;
        self.check_matcher()?;
        match self.effect {
            Effect::Slow { delay_ms, jitter_ms } => {
                if delay_ms == 0 || delay_ms > LONGEST_DELAY_MS || jitter_ms > LONGEST_DELAY_MS {
                    return Err(AddError::BadDelay);
                }
            }
            Effect::Fail { code } => {
                if !matches!(
                    kafka_wire::ResponseError::try_from_code(code),
                    Some(e) if !matches!(e, kafka_wire::ResponseError::Unknown(_))
                ) {
                    return Err(AddError::BadCode);
                }
                if self.matcher.apis.is_empty() {
                    return Err(AddError::FailNeedsApi);
                }
                if let Some(api) = self.matcher.apis.iter().find(|a| !super::can_fail(**a)) {
                    return Err(AddError::CannotFail(*api));
                }
            }
            Effect::Drop => {}
        }
        Ok(())
    }

    fn check_share(&self) -> Result<(), AddError> {
        match self.share {
            Share::EveryNth(n) if !(2..=1000).contains(&n) => Err(AddError::BadShare),
            Share::Percent(p) if !(1..=99).contains(&p) => Err(AddError::BadShare),
            _ => Ok(()),
        }
    }

    fn check_matcher(&self) -> Result<(), AddError> {
        let m = &self.matcher;
        if m.topics.len() > 16 {
            return Err(AddError::TooManyTopics);
        }
        if m.partitions.len() > 64 || m.partitions.iter().any(|p| *p < 0) {
            return Err(AddError::BadPartition);
        }
        Ok(())
    }

    /// The name the rule goes by.
    #[must_use]
    pub fn display_name(&self) -> String {
        if self.name.is_empty() {
            self.effect.word().to_owned()
        } else {
            self.name.clone()
        }
    }

    /// What makes two rules the same one: a second rule with this key replaces the first.
    pub(super) fn same_as(&self, other: &Self) -> bool {
        self.matcher == other.matcher
            && std::mem::discriminant(&self.effect) == std::mem::discriminant(&other.effect)
    }
}

/// The first request a rule fired on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstHit {
    /// When, in milliseconds since the epoch.
    pub at_ms: i64,
    /// The client's `client.id`, clipped.
    pub client: Box<str>,
    /// The API.
    pub api: ApiKey,
    /// The version.
    pub version: i16,
    /// The first topic and partition the request named.
    pub target: Option<(Box<str>, Option<i32>)>,
}

/// The latest request a rule fired on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LastHit {
    /// When, in milliseconds since the epoch.
    pub at_ms: i64,
    /// The connection it arrived on, if the server numbers them.
    pub conn: Option<u64>,
    /// Its correlation id.
    pub correlation: i32,
}

/// A rule in force, with what it has done so far.
#[derive(Debug, Clone)]
pub struct Rule {
    /// Its number: unique for the life of the table, never reused.
    pub id: u32,
    /// What it was asked to be.
    pub spec: Spec,
    /// The name it goes by (the spec's, or the effect's).
    pub name: Arc<str>,
    /// When it was made, in milliseconds since the epoch.
    pub created_ms: i64,
    /// When it ends, in milliseconds since the epoch.
    pub ends_ms: i64,
    /// Whether it fires (a rule switched off still runs out).
    pub enabled: bool,
    /// How many requests fitted it.
    pub matched: u64,
    /// How many it fired on.
    pub hits: u64,
    /// The first request it fired on.
    pub first: Option<FirstHit>,
    /// The latest.
    pub last: Option<LastHit>,
    /// The clients it fired on, in the order they came.
    pub clients: Vec<Box<str>>,
    /// More clients than [`MOST_CLIENTS`].
    pub more_clients: u32,
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
    /// A rule with the same matcher and effect took its place.
    Replaced,
}

/// A rule that is over.
#[derive(Debug, Clone)]
pub struct Ended {
    /// The rule as it stood at the end.
    pub rule: Rule,
    /// When it ended, in milliseconds since the epoch.
    pub at_ms: i64,
    /// Why.
    pub why: EndedWhy,
}

#[cfg(test)]
mod tests;
