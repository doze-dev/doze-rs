//! Event patterns: read, checked, and matched against events.
//!
//! A pattern is a JSON object. Each key names a field of the event, and holds either a nested object (the field is an object, and the
//! same rules apply inside it) or an array of conditions, any one of which may match the field's value. Every key must match; a key
//! `$or` holds an array of objects, one of which must match. An event field that is an array matches when any element does.
//!
//! The conditions are the ones EventBridge documents: exact values (strings, numbers, `true`, `false`, `null`), `prefix`, `suffix`,
//! `equals-ignore-case`, `wildcard`, `anything-but`, `numeric`, `exists` and `cidr`.

mod json;
mod matcher;
mod parse;
mod wildcard;

use std::net::IpAddr;

pub(crate) use matcher::matches;
pub(crate) use parse::parse;

/// A comparison for a number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cmp {
    Eq,
    Gt,
    Ge,
    Lt,
    Le,
}

/// A way to compare text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Text {
    Exact(String),
    Prefix(String),
    Suffix(String),
    /// Prefix compared without regard to case.
    PrefixIgnoreCase(String),
    SuffixIgnoreCase(String),
    EqualsIgnoreCase(String),
    Wildcard(String),
}

/// One condition of an array.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Cond {
    Num(f64),
    Bool(bool),
    Null,
    /// A string, however it is compared.
    Text(Text),
    /// Anything but a number or text in the list.
    AnythingBut(Vec<Cond>),
    /// Every comparison must hold (`[">", 1, "<=", 5]`).
    Numeric(Vec<(Cmp, f64)>),
    Exists(bool),
    Cidr(IpAddr, u8),
}

/// What one key holds.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Node {
    Leaf(Vec<Cond>),
    Nested(Object),
}

/// A set of keys that must all match, and groups of alternatives of which one in each must.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Object {
    pub(crate) keys: Vec<(String, Node)>,
    pub(crate) any_of: Vec<Vec<Object>>,
}

/// A pattern that has been read and checked.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Pattern(pub(crate) Object);

/// Whether `text` is an event pattern this service accepts. Exists so the parser can be fuzzed.
pub(crate) fn check(text: &str) -> bool {
    parse(text).is_ok()
}

#[cfg(test)]
mod tests;
