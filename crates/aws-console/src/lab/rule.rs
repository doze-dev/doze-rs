//! What a fault rule is: which calls it fits, what it does to them, how often, and for how long. Plain values with no clock and no
//! lock; [`super::LabTable`] holds them. Every bound of the Lab is here, with the typed refusal for going past it.

use std::time::Duration;

mod matcher;
mod refuse;

pub use matcher::{Matcher, Pattern};
pub use refuse::{AddError, Field};

/// The kind of an effect: what a table keeps rules of one kind apart by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A wait.
    Slow,
    /// An error.
    Fail,
    /// A closed connection.
    Drop,
}

/// The most rules in force at once.
pub const MOST_RULES: usize = 50;
/// The most ended rules kept.
pub const MOST_ENDED: usize = 200;
/// The longest a rule may last.
pub const LONGEST_RULE: Duration = Duration::from_secs(24 * 3600);
/// The shortest a rule may last.
pub const SHORTEST_RULE: Duration = Duration::from_secs(1);
/// The longest delay a rule may add (and jitter).
pub const LONGEST_DELAY_MS: u32 = 60_000;
/// The longest a rule's name is, in characters.
pub const LONGEST_NAME: usize = 80;
/// The longest a resource pattern is, in bytes.
pub const LONGEST_PATTERN: usize = 256;
/// The longest a service, operation, region or access key word is, in bytes.
pub const LONGEST_WORD: usize = 128;
/// The longest a custom error code is, in characters.
pub const LONGEST_CODE: usize = 64;
/// The most services, operations and resource patterns a rule may name.
pub const MOST_SERVICES: usize = 16;
/// See [`MOST_SERVICES`].
pub const MOST_OPERATIONS: usize = 16;
/// See [`MOST_SERVICES`].
pub const MOST_RESOURCES: usize = 16;
/// The most regions and access keys a rule may name.
pub const MOST_REGIONS: usize = 8;
/// See [`MOST_REGIONS`].
pub const MOST_CALLERS: usize = 8;
/// The most callers a rule remembers having fired on; the hits of the rest are counted.
pub const MOST_KEPT_CALLERS: usize = 8;
/// The smallest and largest `n` of "every Nth".
pub const NTH: std::ops::RangeInclusive<u32> = 2..=1000;
/// The smallest and largest chance, in a hundred.
pub const PERCENT: std::ops::RangeInclusive<u8> = 1..=99;

/// An error a Fail rule answers with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fail {
    /// The service's throttling error.
    Throttle,
    /// 503, the service is unavailable.
    Unavailable,
    /// 500, the service failed inside.
    Internal,
    /// The caller may not do this.
    AccessDenied,
    /// An error the operation's model lists, which the person chose: the status and code the model gives, and the model's shape
    /// (so the gateway words it as the model says). A rule not made through the console's API may carry a code of its own and no shape.
    Custom {
        status: u16,
        code: Box<str>,
        shape: Option<&'static str>,
    },
}

/// What a rule does to a call it fires on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Wait `delay_ms` and a random 0 to `jitter_ms`, then serve it.
    Slow { delay_ms: u32, jitter_ms: u32 },
    /// Answer with an error without asking the service.
    Fail(Fail),
    /// Close the connection with no answer.
    Drop,
}

impl Effect {
    /// The kind of effect: two rules of one kind for the same calls are one rule.
    #[must_use]
    pub fn kind(&self) -> Kind {
        match self {
            Self::Slow { .. } => Kind::Slow,
            Self::Fail(_) => Kind::Fail,
            Self::Drop => Kind::Drop,
        }
    }
}

/// How many of the calls a rule fits it fires on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Share {
    /// Every one.
    Every,
    /// Every Nth: the Nth, the 2Nth...
    Nth(u32),
    /// A chance of this many in a hundred, drawn from the table's seeded generator.
    Percent(u8),
}

/// A rule as it is asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    /// What the rule is called; empty is named by what it does.
    pub name: String,
    /// Which calls it fits.
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

impl Spec {
    /// Everything wrong with the rule that its parts alone can tell (the table adds "full" and the API adds the names no service has).
    #[must_use]
    pub fn problems(&self) -> Vec<AddError> {
        let mut out = Vec::new();
        if self.name.chars().count() > LONGEST_NAME || self.name.chars().any(char::is_control) {
            out.push(AddError::BadName);
        }
        if !(SHORTEST_RULE..=LONGEST_RULE).contains(&self.ttl) {
            out.push(AddError::BadDuration);
        }
        if self.max_hits == Some(0) {
            out.push(AddError::BadHits);
        }
        match self.share {
            Share::Nth(n) if !NTH.contains(&n) => out.push(AddError::BadShare),
            Share::Percent(p) if !PERCENT.contains(&p) => out.push(AddError::BadShare),
            _ => {}
        }
        out.extend(self.matcher.too_many());
        match &self.effect {
            Effect::Slow {
                delay_ms,
                jitter_ms,
            } if *delay_ms == 0
                || *delay_ms > LONGEST_DELAY_MS
                || *jitter_ms > LONGEST_DELAY_MS =>
            {
                out.push(AddError::BadDelay);
            }
            Effect::Fail(Fail::Custom { status, code, .. }) => {
                if !(400..=599).contains(status) {
                    out.push(AddError::BadStatus(*status));
                }
                let ok = !code.is_empty()
                    && code.chars().count() <= LONGEST_CODE
                    && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '.');
                if !ok {
                    out.push(AddError::BadCode);
                }
            }
            _ => {}
        }
        out
    }

    /// The first thing wrong with the rule.
    ///
    /// # Errors
    /// [`Spec::problems`], the first.
    pub fn check(&self) -> Result<(), AddError> {
        self.problems().into_iter().next().map_or(Ok(()), Err)
    }

    /// What makes two rules the same one: the same calls and the same kind of effect (a second rule replaces the first).
    pub(super) fn same_as(&self, other: &Self) -> bool {
        self.matcher == other.matcher && self.effect.kind() == other.effect.kind()
    }
}

#[cfg(test)]
mod tests;
