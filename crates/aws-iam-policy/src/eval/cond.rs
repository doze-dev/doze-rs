//! Condition operators, as IAM defines them (policy-elements-condition-operators): string, numeric, date, boolean, binary, IP address,
//! ARN, and `Null`; the set modifiers `ForAllValues:` and `ForAnyValue:`; the `IfExists` suffix; and the rule that a key the request
//! lacks makes a condition false, except under `Null`, `IfExists`, and the negated operators.

use std::net::IpAddr;

use aws_core::time::parse_rfc3339;

use super::context::Context;
use super::glob;
use super::vars::substitute;
use crate::document::Condition;

/// What a condition came to.
pub(super) struct Outcome {
    pub(super) met: bool,
    /// A key AWS supplies and this emulator does not, which the answer hangs on.
    pub(super) unsupplied: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Set {
    None,
    All,
    Any,
}

pub(super) fn holds(c: &Condition, ctx: &Context) -> Outcome {
    let (set, rest) = if let Some(r) = c.operator.strip_prefix("ForAllValues:") {
        (Set::All, r)
    } else if let Some(r) = c.operator.strip_prefix("ForAnyValue:") {
        (Set::Any, r)
    } else {
        (Set::None, c.operator.as_str())
    };
    let (base, if_exists) = rest
        .strip_suffix("IfExists")
        .map_or((rest, false), |b| (b, true));
    let policy: Vec<String> = c.values.iter().filter_map(|v| substitute(v, ctx)).collect();
    let met = match ctx.get(&c.key) {
        None => absent(base, set, if_exists, &policy),
        Some(have) => present(base, set, have, &policy),
    };
    let unsupplied =
        (!met && ctx.get(&c.key).is_none() && ctx.is_unsupplied(&c.key)).then(|| c.key.clone());
    Outcome { met, unsupplied }
}

/// A condition on a key the request does not carry.
fn absent(base: &str, set: Set, if_exists: bool, policy: &[String]) -> bool {
    if base == "Null" {
        return policy.iter().any(|v| v.eq_ignore_ascii_case("true"));
    }
    match set {
        Set::All => true,
        Set::Any => false,
        Set::None => if_exists || negated(base),
    }
}

fn negated(base: &str) -> bool {
    matches!(
        base,
        "StringNotEquals"
            | "StringNotEqualsIgnoreCase"
            | "StringNotLike"
            | "NumericNotEquals"
            | "DateNotEquals"
            | "NotIpAddress"
            | "ArnNotEquals"
            | "ArnNotLike"
    )
}

/// A condition on a key the request carries, with its values `have`.
fn present(base: &str, set: Set, have: &[String], policy: &[String]) -> bool {
    if base == "Null" {
        return policy.iter().any(|v| v.eq_ignore_ascii_case("false"));
    }
    let one = |value: &str| {
        let any = policy.iter().any(|p| compare(positive(base), value, p));
        if negated(base) { !any } else { any }
    };
    match set {
        Set::All => have.iter().all(|v| one(v)),
        Set::Any | Set::None => have.iter().any(|v| one(v)),
    }
}

/// The operator a negated one is the opposite of.
fn positive(base: &str) -> &str {
    match base {
        "StringNotEquals" => "StringEquals",
        "StringNotEqualsIgnoreCase" => "StringEqualsIgnoreCase",
        "StringNotLike" => "StringLike",
        "NumericNotEquals" => "NumericEquals",
        "DateNotEquals" => "DateEquals",
        "NotIpAddress" => "IpAddress",
        "ArnNotEquals" => "ArnEquals",
        "ArnNotLike" => "ArnLike",
        other => other,
    }
}

fn compare(op: &str, have: &str, want: &str) -> bool {
    match op {
        "StringEquals" => have == want,
        "StringEqualsIgnoreCase" => have.eq_ignore_ascii_case(want),
        "StringLike" => glob::matches(want, have, false),
        "Bool" => have.eq_ignore_ascii_case(want),
        "BinaryEquals" => have == want,
        "ArnEquals" | "ArnLike" => arn_like(want, have),
        "IpAddress" => ip_in(have, want),
        "NumericEquals"
        | "NumericLessThan"
        | "NumericLessThanEquals"
        | "NumericGreaterThan"
        | "NumericGreaterThanEquals" => match (have.parse::<f64>(), want.parse::<f64>()) {
            (Ok(a), Ok(b)) => ordered(op.trim_start_matches("Numeric"), a.partial_cmp(&b)),
            _ => false,
        },
        "DateEquals"
        | "DateLessThan"
        | "DateLessThanEquals"
        | "DateGreaterThan"
        | "DateGreaterThanEquals" => match (when(have), when(want)) {
            (Some(a), Some(b)) => ordered(op.trim_start_matches("Date"), Some(a.cmp(&b))),
            _ => false,
        },
        _ => false,
    }
}

fn ordered(rel: &str, o: Option<std::cmp::Ordering>) -> bool {
    use std::cmp::Ordering::{Equal, Greater, Less};
    matches!(
        (rel, o),
        ("Equals", Some(Equal))
            | ("LessThan", Some(Less))
            | ("LessThanEquals", Some(Less | Equal))
            | ("GreaterThan", Some(Greater))
            | ("GreaterThanEquals", Some(Greater | Equal))
    )
}

/// An instant as seconds: epoch seconds, or an ISO 8601 / RFC 3339 time.
fn when(s: &str) -> Option<i64> {
    s.parse::<f64>()
        .ok()
        .map(|f| f.floor() as i64)
        .or_else(|| parse_rfc3339(s))
}

/// `ArnLike`: six colon-separated parts, each matched as a wildcard pattern (so a `*` does not cross a colon).
fn arn_like(pattern: &str, arn: &str) -> bool {
    let (p, a): (Vec<&str>, Vec<&str>) = (
        pattern.splitn(6, ':').collect(),
        arn.splitn(6, ':').collect(),
    );
    p.len() == a.len() && p.iter().zip(&a).all(|(p, a)| glob::matches(p, a, false))
}

/// Whether `ip` is in `cidr` (or is the single address `cidr`).
fn ip_in(ip: &str, cidr: &str) -> bool {
    let Ok(ip) = ip.parse::<IpAddr>() else {
        return false;
    };
    let (net, bits) = cidr
        .split_once('/')
        .map_or((cidr, None), |(n, b)| (n, b.parse::<u32>().ok()));
    let Ok(net) = net.parse::<IpAddr>() else {
        return false;
    };
    match (ip, net) {
        (IpAddr::V4(a), IpAddr::V4(b)) => prefix_equal(
            u128::from(u32::from(a)),
            u128::from(u32::from(b)),
            32,
            bits.unwrap_or(32),
        ),
        (IpAddr::V6(a), IpAddr::V6(b)) => {
            prefix_equal(u128::from(a), u128::from(b), 128, bits.unwrap_or(128))
        }
        _ => false,
    }
}

fn prefix_equal(a: u128, b: u128, width: u32, bits: u32) -> bool {
    if bits > width {
        return false;
    }
    let shift = width - bits;
    shift >= 128 || (a >> shift) == (b >> shift)
}
