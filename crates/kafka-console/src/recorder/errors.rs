//! The error table: every error code the broker serves, with Kafka's own name and words, one plain sentence, and a class.
//!
//! Classes: `Ok` (no error), `Warn` (expected and the client handles it: a rebalance, a coordinator that moved), `Bad` (the
//! broker failed), `Refused` (Kafka would refuse it too: violet), `Denied` (an ACL or a login said no: amber). A test walks
//! the broker's source for every `ResponseError` it names and fails on a code that has no row here, and another checks every
//! row's name against the protocol crate's own.

mod table_a;
mod table_b;

/// How a result is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    Ok,
    Warn,
    Bad,
    Refused,
    Denied,
}

/// One error code.
#[derive(Debug, Clone, Copy)]
pub struct Row {
    pub code: i16,
    /// Kafka's name for it, as clients print it.
    pub name: &'static str,
    pub class: Class,
    /// Kafka's own words for it.
    pub words: &'static str,
    /// What it means, in a plain sentence of ours.
    pub plain: &'static str,
    /// What would pass, where there is one thing to say.
    pub pass: &'static str,
}

pub(crate) const fn row(
    code: i16,
    name: &'static str,
    class: Class,
    words: &'static str,
    plain: &'static str,
    pass: &'static str,
) -> Row {
    Row {
        code,
        name,
        class,
        words,
        plain,
        pass,
    }
}

pub(crate) use Class::{Bad, Denied, Refused, Warn};
pub(crate) use row as r;

/// Every row, in code order within each half.
pub fn all() -> impl Iterator<Item = &'static Row> {
    table_a::ROWS.iter().chain(table_b::ROWS.iter())
}

/// The row of a code; `None` for 0 and for a code this broker never serves.
#[must_use]
pub fn of(code: i16) -> Option<&'static Row> {
    all().find(|r| r.code == code)
}

/// The class of a code: zero is `Ok`, a code with no row is `Bad` (the broker said something it has no words for).
#[must_use]
pub fn class(code: i16) -> Class {
    if code == 0 {
        Class::Ok
    } else {
        of(code).map_or(Class::Bad, |r| r.class)
    }
}

/// Kafka's name for a code: `NONE` for zero, a row's name, or `ERROR_<code>` for one with no row.
#[must_use]
pub fn name(code: i16) -> String {
    if code == 0 {
        "NONE".to_owned()
    } else {
        of(code).map_or_else(|| format!("ERROR_{code}"), |r| r.name.to_owned())
    }
}

#[cfg(test)]
mod tests;
