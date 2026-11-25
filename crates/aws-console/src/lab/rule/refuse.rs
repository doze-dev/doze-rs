//! Why a rule was refused: one typed reason for each thing wrong, with the field it is about and the words that say what to fix.

use super::{
    LONGEST_CODE, LONGEST_DELAY_MS, LONGEST_NAME, LONGEST_PATTERN, LONGEST_WORD, MOST_CALLERS,
    MOST_OPERATIONS, MOST_REGIONS, MOST_RESOURCES, MOST_RULES, MOST_SERVICES, NTH, PERCENT,
};

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

    pub(super) const fn most(self) -> usize {
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
            Self::TooMany(f) => format!("A rule names at most {} {}.", f.most(), f.word()),
            Self::BadEntry(f) => format!(
                "Each of the {} is 1 to {} characters with no control character in it.",
                f.word(),
                if *f == Field::Resources {
                    LONGEST_PATTERN
                } else {
                    LONGEST_WORD
                }
            ),
            Self::UnknownService(s) => format!("doze-aws has no service called {s}."),
            Self::UnknownOperation(o) => {
                format!("None of the services this rule names has an operation called {o}.")
            }
        }
    }
}
