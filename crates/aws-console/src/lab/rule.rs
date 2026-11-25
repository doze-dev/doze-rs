//! What a fault rule is: which calls it fits, what it does to them, how often, and for how long. Plain values with no clock and no
//! lock; [`super::LabTable`] holds them. Every bound of the Lab is here, with the typed refusal for going past it.

use std::time::Duration;

use super::glob;

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

/// A pattern of a rule's resource list ([`glob`]).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pattern(Box<str>);

impl Pattern {
    /// A pattern, or `None` when it is empty, too long, or holds a control character.
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

    /// Whether the resource a call named fits: the whole text, or its last name.
    #[must_use]
    pub fn fits(&self, resource: &str) -> bool {
        glob::fits(&self.0, resource) || glob::fits(&self.0, glob::last_name(resource))
    }
}

/// Which calls a rule fits. Every list that is not empty must fit; an empty one fits anything. Each list is sorted and without
/// repeats, so two rules for the same calls are equal whatever order they were written in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Matcher {
    /// Signing names, SDK ids (compared without regard to case).
    pub services: Vec<Box<str>>,
    /// Operations, exactly.
    pub operations: Vec<Box<str>>,
    /// Regions, exactly (lower case).
    pub regions: Vec<Box<str>>,
    /// Access key ids, exactly.
    pub callers: Vec<Box<str>>,
    /// What a call names.
    pub resources: Vec<Pattern>,
}

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
    /// A status and a code of the person's.
    Custom { status: u16, code: Box<str> },
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

/// Why a rule was refused: one reason per thing wrong, each with the field it is about and the words that say what to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddError {
    /// There are already [`MOST_RULES`] in force (or the Lab is off).
    Full,
    /// The Lab is off (`--lab=false`, or the console is served where nobody asked for it).
    Off(String),
    /// The name is longer than [`LONGEST_NAME`] or holds a control character.
    BadName,
    /// The duration is outside [`SHORTEST_RULE`]..=[`LONGEST_RULE`].
    BadDuration,
    /// A delay of nothing, or a delay or jitter above [`LONGEST_DELAY_MS`].
    BadDelay,
    /// A share outside its range.
    BadShare,
    /// `max_hits` of 0.
    BadHits,
    /// A custom status outside 400 to 599.
    BadStatus(u16),
    /// A custom code that is empty, longer than [`LONGEST_CODE`], or holds a character other than a letter, a digit or a dot.
    BadCode,
    /// More entries than a list may hold.
    TooMany(Field),
    /// An entry that is empty, too long, or holds a control character.
    BadEntry(Field),
    /// A service no doze-aws has.
    UnknownService(String),
    /// An operation none of the services named has.
    UnknownOperation(String),
}

/// A list of a matcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Services,
    Operations,
    Regions,
    Callers,
    Resources,
}

impl Field {
    /// The member of the API body this is.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Services => "services",
            Self::Operations => "operations",
            Self::Regions => "regions",
            Self::Callers => "callers",
            Self::Resources => "resources",
        }
    }

    const fn most(self) -> usize {
        match self {
            Self::Services => MOST_SERVICES,
            Self::Operations => MOST_OPERATIONS,
            Self::Regions => MOST_REGIONS,
            Self::Callers => MOST_CALLERS,
            Self::Resources => MOST_RESOURCES,
        }
    }
}

impl AddError {
    /// The field of the form this is about (the API's `problems[].field`).
    #[must_use]
    pub fn field(&self) -> &'static str {
        match self {
            Self::Full | Self::Off(_) => "rules",
            Self::BadName => "name",
            Self::BadDuration => "ends_in_secs",
            Self::BadDelay | Self::BadStatus(_) | Self::BadCode => "effect",
            Self::BadShare => "share",
            Self::BadHits => "max_hits",
            Self::TooMany(f) | Self::BadEntry(f) => f.word(),
            Self::UnknownService(_) => "services",
            Self::UnknownOperation(_) => "operations",
        }
    }

    /// What is wrong and what to do, in a sentence.
    #[must_use]
    pub fn words(&self) -> String {
        match self {
            Self::Full => format!(
                "There are already {MOST_RULES} rules in force. End one, or use Stop all, to make room."
            ),
            Self::Off(why) => why.clone(),
            Self::BadName => format!(
                "A name is at most {LONGEST_NAME} characters with no control character in it."
            ),
            Self::BadDuration => "A rule lasts from 1 second to 24 hours.".to_owned(),
            Self::BadDelay => format!(
                "A delay is from 1 to {LONGEST_DELAY_MS} milliseconds, and so is the jitter."
            ),
            Self::BadShare => format!(
                "Every Nth takes N from {} to {}, and a chance takes a number from {} to {} in a hundred.",
                NTH.start(),
                NTH.end(),
                PERCENT.start(),
                PERCENT.end()
            ),
            Self::BadHits => "A number of hits is at least 1.".to_owned(),
            Self::BadStatus(s) => format!("{s} is not an error status: use 400 to 599."),
            Self::BadCode => format!(
                "An error code is 1 to {LONGEST_CODE} letters, digits and dots (such as ThrottlingException)."
            ),
            Self::TooMany(f) => format!(
                "A rule names at most {} {}.",
                f.most(),
                f.word()
            ),
            Self::BadEntry(f) => format!(
                "Each of the {} is 1 to {} characters with no control character in it.",
                f.word(),
                if *f == Field::Resources { LONGEST_PATTERN } else { LONGEST_WORD }
            ),
            Self::UnknownService(s) => format!("doze-aws has no service called {s}."),
            Self::UnknownOperation(o) => {
                format!("None of the services this rule names has an operation called {o}.")
            }
        }
    }
}

/// Sorts a list and drops what repeats.
fn tidy<T: Ord>(mut list: Vec<T>) -> Vec<T> {
    list.sort();
    list.dedup();
    list
}

fn word_ok(w: &str) -> bool {
    !w.is_empty() && w.len() <= LONGEST_WORD && !w.chars().any(char::is_control)
}

impl Matcher {
    /// A matcher from the words as typed. Each list is made tidy; an entry that is not one is left out and said (the matcher is then
    /// what the rest of the words mean, so a sentence can still be made of it).
    #[must_use]
    pub fn build(
        services: &[String],
        operations: &[String],
        regions: &[String],
        callers: &[String],
        resources: &[String],
    ) -> (Self, Vec<AddError>) {
        let mut problems = Vec::new();
        let mut words = |field: Field, list: &[String], lower: bool| -> Vec<Box<str>> {
            let mut kept = Vec::new();
            for w in list {
                let w = w.trim();
                if word_ok(w) {
                    kept.push(if lower { w.to_ascii_lowercase() } else { w.to_owned() }.into());
                } else {
                    problems.push(AddError::BadEntry(field));
                }
            }
            kept
        };
        let services = words(Field::Services, services, true);
        let operations = words(Field::Operations, operations, false);
        let regions = words(Field::Regions, regions, true);
        let callers = words(Field::Callers, callers, false);
        let mut patterns = Vec::new();
        for r in resources {
            match Pattern::new(r.trim()) {
                Some(p) => patterns.push(p),
                None => problems.push(AddError::BadEntry(Field::Resources)),
            }
        }
        let matcher = Self {
            services: tidy(services),
            operations: tidy(operations),
            regions: tidy(regions),
            callers: tidy(callers),
            resources: tidy(patterns),
        };
        problems.extend(matcher.too_many());
        problems.dedup();
        (matcher, problems)
    }

    fn too_many(&self) -> Vec<AddError> {
        [
            (Field::Services, self.services.len()),
            (Field::Operations, self.operations.len()),
            (Field::Regions, self.regions.len()),
            (Field::Callers, self.callers.len()),
            (Field::Resources, self.resources.len()),
        ]
        .into_iter()
        .filter(|(f, n)| *n > f.most())
        .map(|(f, _)| AddError::TooMany(f))
        .collect()
    }

    /// Whether every list that is set fits a call.
    #[must_use]
    pub fn fits(&self, ask: &aws_gateway::Ask<'_>) -> bool {
        let named = |list: &[Box<str>], what: &str| list.is_empty() || list.iter().any(|w| **w == *what);
        (self.services.is_empty()
            || self.services.iter().any(|s| {
                s.eq_ignore_ascii_case(ask.signing_name) || s.eq_ignore_ascii_case(ask.sdk_id)
            }))
            && named(&self.operations, ask.operation)
            && named(&self.regions, ask.region)
            && named(&self.callers, ask.access_key_id)
            && (self.resources.is_empty()
                || (!ask.resource.is_empty()
                    && self.resources.iter().any(|p| p.fits(ask.resource))))
    }
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
            } if *delay_ms == 0 || *delay_ms > LONGEST_DELAY_MS || *jitter_ms > LONGEST_DELAY_MS => {
                out.push(AddError::BadDelay);
            }
            Effect::Fail(Fail::Custom { status, code }) => {
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
