//! The key condition of a query: an equality on the partition key, and at most one condition on the sort key.

use std::cmp::Ordering;

use crate::Invalid;
use crate::ast::{Cmp, Cond, Elem, Func, Operand};
use crate::parse_cond::parse_condition;
use crate::scope::{Kind, Scope};
use crate::value::Value;

/// A condition on the sort key.
#[derive(Clone, Debug, PartialEq)]
pub enum Range {
    /// `sk = :v`
    Eq(Value),
    /// `sk < :v`
    Lt(Value),
    /// `sk <= :v`
    Le(Value),
    /// `sk > :v`
    Gt(Value),
    /// `sk >= :v`
    Ge(Value),
    /// `sk BETWEEN :a AND :b`
    Between(Value, Value),
    /// `begins_with(sk, :v)`
    BeginsWith(Value),
}

impl Range {
    /// Whether a sort key value satisfies the condition.
    #[must_use]
    pub fn matches(&self, v: &Value) -> bool {
        let cmp = |b: &Value| v.compare(b);
        match self {
            Self::Eq(b) => cmp(b) == Some(Ordering::Equal),
            Self::Lt(b) => cmp(b).is_some_and(Ordering::is_lt),
            Self::Le(b) => cmp(b).is_some_and(Ordering::is_le),
            Self::Gt(b) => cmp(b).is_some_and(Ordering::is_gt),
            Self::Ge(b) => cmp(b).is_some_and(Ordering::is_ge),
            Self::Between(a, b) => {
                cmp(a).is_some_and(Ordering::is_ge) && cmp(b).is_some_and(Ordering::is_le)
            }
            Self::BeginsWith(p) => match (v, p) {
                (Value::S(s), Value::S(p)) => s.starts_with(p.as_str()),
                (Value::B(s), Value::B(p)) => s.starts_with(p),
                _ => false,
            },
        }
    }

    /// Where an ascending read of the partition may begin (`None`: at its start).
    #[must_use]
    pub fn seek_from(&self) -> Option<&Value> {
        match self {
            Self::Eq(v) | Self::Gt(v) | Self::Ge(v) | Self::Between(v, _) | Self::BeginsWith(v) => {
                Some(v)
            }
            Self::Lt(_) | Self::Le(_) => None,
        }
    }

    /// Where a descending read may begin (`None`: at the partition's end).
    #[must_use]
    pub fn seek_to(&self) -> Option<&Value> {
        match self {
            Self::Eq(v) | Self::Lt(v) | Self::Le(v) | Self::Between(_, v) => Some(v),
            Self::Gt(_) | Self::Ge(_) | Self::BeginsWith(_) => None,
        }
    }

    /// Whether, reading ascending, no value after `v` can match.
    #[must_use]
    pub fn done_after(&self, v: &Value) -> bool {
        let past = |b: &Value, strict: bool| {
            v.compare(b)
                .is_some_and(|o| if strict { o.is_ge() } else { o.is_gt() })
        };
        match self {
            Self::Eq(b) | Self::Le(b) | Self::Between(_, b) => past(b, false),
            Self::Lt(b) => past(b, true),
            Self::BeginsWith(_) => {
                v.compare(self.seek_from().unwrap_or(v))
                    .is_some_and(Ordering::is_gt)
                    && !self.matches(v)
            }
            Self::Gt(_) | Self::Ge(_) => false,
        }
    }

    /// Whether, reading descending, no value before `v` can match.
    #[must_use]
    pub fn done_before(&self, v: &Value) -> bool {
        let below = |b: &Value, strict: bool| {
            v.compare(b)
                .is_some_and(|o| if strict { o.is_le() } else { o.is_lt() })
        };
        match self {
            Self::Eq(b) | Self::Ge(b) | Self::Between(b, _) => below(b, false),
            Self::Gt(b) => below(b, true),
            Self::BeginsWith(p) => v.compare(p).is_some_and(Ordering::is_lt) && !self.matches(v),
            Self::Lt(_) | Self::Le(_) => false,
        }
    }
}

/// A parsed key condition.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyCondition {
    /// The partition key's value.
    pub hash: Value,
    /// The condition on the sort key, if there is one.
    pub range: Option<Range>,
}

/// Reads and checks a key condition against the key attributes `hash` and `range` (each with its scalar type: `S`, `N` or `B`).
///
/// # Errors
/// AWS's refusal of the text or of what it asks.
pub fn parse_key_condition(
    text: &str,
    scope: &mut Scope<'_>,
    hash: (&str, &str),
    range: Option<(&str, &str)>,
) -> Result<KeyCondition, Invalid> {
    let cond = parse_condition(text, Kind::KeyCondition, scope)?;
    let terms = match cond {
        Cond::And(a, b) => vec![*a, *b],
        one => vec![one],
    };
    let mut found: Vec<(String, Range)> = Vec::new();
    for t in terms {
        found.push(term(t)?);
    }
    build(found, hash, range)
}

fn unsupported() -> Invalid {
    Invalid("Query key condition not supported".to_owned())
}

/// A name and the condition on it.
fn term(c: Cond) -> Result<(String, Range), Invalid> {
    match c {
        Cond::Cmp(op, Operand::Path(p), Operand::Value(v)) => {
            let name = single(&p.0)?;
            let r = match op {
                Cmp::Eq => Range::Eq(v),
                Cmp::Lt => Range::Lt(v),
                Cmp::Le => Range::Le(v),
                Cmp::Gt => Range::Gt(v),
                Cmp::Ge => Range::Ge(v),
                Cmp::Ne => return Err(unsupported()),
            };
            Ok((name, r))
        }
        Cond::Between(Operand::Path(p), Operand::Value(a), Operand::Value(b)) => {
            Ok((single(&p.0)?, Range::Between(a, b)))
        }
        Cond::Func(Func::BeginsWith, args) => match <[Operand; 2]>::try_from(args) {
            Ok([Operand::Path(p), Operand::Value(v)]) => Ok((single(&p.0)?, Range::BeginsWith(v))),
            _ => Err(unsupported()),
        },
        _ => Err(unsupported()),
    }
}

fn single(elems: &[Elem]) -> Result<String, Invalid> {
    match elems {
        [Elem::Key(k)] => Ok(k.clone()),
        _ => Err(unsupported()),
    }
}

fn build(
    found: Vec<(String, Range)>,
    hash: (&str, &str),
    range: Option<(&str, &str)>,
) -> Result<KeyCondition, Invalid> {
    let mut hash_value = None;
    let mut sort = None;
    for (name, r) in found {
        let slot = if name == hash.0 {
            if !matches!(r, Range::Eq(_)) {
                return Err(unsupported());
            }
            check_type(&r, hash.1)?;
            &mut hash_value
        } else if range.is_some_and(|(n, _)| n == name) {
            if let Some((_, ty)) = range {
                check_type(&r, ty)?;
            }
            &mut sort
        } else {
            return Err(unsupported());
        };
        if slot.is_some() {
            return Err(Kind::KeyCondition
                .invalid("KeyConditionExpressions must only contain one condition per key"));
        }
        *slot = Some(r);
    }
    let Some(Range::Eq(hash_value)) = hash_value else {
        return Err(Invalid(format!(
            "Query condition missed key schema element: {}",
            hash.0
        )));
    };
    Ok(KeyCondition {
        hash: hash_value,
        range: sort,
    })
}

/// Every operand of the condition has the type of the key attribute it is held against.
fn check_type(r: &Range, ty: &str) -> Result<(), Invalid> {
    let operands: Vec<&Value> = match r {
        Range::Eq(v)
        | Range::Lt(v)
        | Range::Le(v)
        | Range::Gt(v)
        | Range::Ge(v)
        | Range::BeginsWith(v) => vec![v],
        Range::Between(a, b) => vec![a, b],
    };
    if let Range::BeginsWith(v) = r
        && (ty == "N" || !matches!(v, Value::S(_) | Value::B(_)))
    {
        // `(memory)`: begins_with cannot be used with a sort key of type Number (the model's documentation says so).
        let shown = if ty == "N" { "N" } else { v.type_name() };
        return Err(Kind::KeyCondition.invalid(&format!(
            "Incorrect operand type for operator or function; operator or function: begins_with, operand type: {shown}"
        )));
    }
    if operands.iter().any(|v| v.type_name() != ty) {
        return Err(Invalid("One or more parameter values were invalid: Condition parameter type does not match schema type".to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{NameMap, ValueMap};
    use crate::number::Number;

    fn parse(text: &str) -> Result<KeyCondition, String> {
        let values: ValueMap = [
            (":p".to_owned(), Value::S("a".into())),
            (":lo".to_owned(), Value::N(Number::from_i64(2))),
            (":hi".to_owned(), Value::N(Number::from_i64(4))),
            (":s".to_owned(), Value::S("x".into())),
        ]
        .into();
        let names: NameMap = [("#o".to_owned(), "pk".to_owned())].into();
        parse_key_condition(
            text,
            &mut Scope::new(&names, &values),
            ("pk", "S"),
            Some(("sk", "N")),
        )
        .map_err(|e| e.0)
    }

    #[test]
    fn shapes() {
        assert!(parse("pk = :p").unwrap().range.is_none());
        assert!(
            parse("#o = :p AND sk BETWEEN :lo AND :hi")
                .unwrap()
                .range
                .is_some()
        );
        assert!(parse("sk >= :lo AND pk = :p").unwrap().range.is_some());
    }

    #[test]
    fn refusals() {
        assert_eq!(
            parse("sk = :lo").unwrap_err(),
            "Query condition missed key schema element: pk"
        );
        assert_eq!(
            parse("pk = :p OR sk = :lo").unwrap_err(),
            "Query key condition not supported"
        );
        assert_eq!(
            parse("pk <> :p").unwrap_err(),
            "Query key condition not supported"
        );
        assert!(parse("pk = :p AND sk = :lo AND sk = :hi").is_err());
        assert!(
            parse("pk = :p AND sk = :s")
                .unwrap_err()
                .contains("Condition parameter type does not match schema type")
        );
        assert!(
            parse("pk = :p AND begins_with(sk, :s)")
                .unwrap_err()
                .contains("begins_with")
        );
        assert!(
            parse("pk = :p AND pk = :p")
                .unwrap_err()
                .contains("one condition per key")
        );
    }

    #[test]
    fn ranges_match_and_bound_the_scan() {
        let n = |i| Value::N(Number::from_i64(i));
        let r = Range::Between(n(2), n(4));
        assert!(r.matches(&n(3)) && !r.matches(&n(5)));
        assert!(r.done_after(&n(5)) && !r.done_after(&n(4)));
        assert!(r.done_before(&n(1)) && !r.done_before(&n(2)));
        assert_eq!(r.seek_from(), Some(&n(2)));
        let p = Range::BeginsWith(Value::S("ab".into()));
        assert!(p.done_after(&Value::S("ac".into())) && !p.done_after(&Value::S("ab1".into())));
    }
}
