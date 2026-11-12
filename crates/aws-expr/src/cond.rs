//! Evaluating a condition or filter against an item.
//!
//! A path that is not in the item has no value; a comparison with no value is false, except `<>` (true) and what `NOT` makes of
//! false. A comparison between two types is false except `<>`. `(memory)` for `<>` with a missing attribute: AWS's documentation
//! says only "true if a is not equal to b".

use std::borrow::Cow;
use std::cmp::Ordering;

use crate::ast::{Cmp, Cond, Func, Operand};
use crate::number::Number;
use crate::path::get;
use crate::value::{Item, Value};

impl Cond {
    /// Whether the item satisfies the condition. An absent item is the empty one.
    #[must_use]
    pub fn eval(&self, item: &Item) -> bool {
        match self {
            Self::And(a, b) => a.eval(item) && b.eval(item),
            Self::Or(a, b) => a.eval(item) || b.eval(item),
            Self::Not(a) => !a.eval(item),
            Self::Cmp(op, a, b) => compare(
                *op,
                operand(a, item).as_deref(),
                operand(b, item).as_deref(),
            ),
            Self::Between(v, lo, hi) => {
                let (v, lo, hi) = (operand(v, item), operand(lo, item), operand(hi, item));
                match (v.as_deref(), lo.as_deref(), hi.as_deref()) {
                    (Some(v), Some(lo), Some(hi)) => {
                        v.compare(lo).is_some_and(Ordering::is_ge)
                            && v.compare(hi).is_some_and(Ordering::is_le)
                    }
                    _ => false,
                }
            }
            Self::In(v, list) => {
                let v = operand(v, item);
                v.as_deref()
                    .is_some_and(|v| list.iter().any(|o| operand(o, item).as_deref() == Some(v)))
            }
            Self::Func(f, args) => function(*f, args, item),
        }
    }
}

impl Cond {
    /// The top-level attributes the condition looks at, with repeats.
    #[must_use]
    pub fn roots(&self) -> Vec<&str> {
        match self {
            Self::And(a, b) | Self::Or(a, b) => a.roots().into_iter().chain(b.roots()).collect(),
            Self::Not(a) => a.roots(),
            Self::Cmp(_, a, b) => [a, b].into_iter().filter_map(root_of).collect(),
            Self::Between(a, b, c) => [a, b, c].into_iter().filter_map(root_of).collect(),
            Self::In(a, rest) => std::iter::once(a).chain(rest).filter_map(root_of).collect(),
            Self::Func(_, args) => args.iter().filter_map(root_of).collect(),
        }
    }
}

/// The value an operand stands for in `item`.
pub(crate) fn operand<'a>(op: &'a Operand, item: &'a Item) -> Option<Cow<'a, Value>> {
    match op {
        Operand::Value(v) => Some(Cow::Borrowed(v)),
        Operand::Path(p) => get(item, p).map(Cow::Borrowed),
        Operand::Size(p) => get(item, p)
            .and_then(size)
            .map(|n| Cow::Owned(Value::N(Number::from_i64(n)))),
        // WHY: these two exist only in update expressions, which the update evaluator reads itself.
        Operand::IfNotExists(..) | Operand::ListAppend(..) => None,
    }
}

/// `size(...)`: characters, bytes, or elements; the other types have none.
fn size(v: &Value) -> Option<i64> {
    let n = match v {
        Value::S(s) => s.chars().count(),
        Value::B(b) => b.len(),
        Value::Ss(s) => s.len(),
        Value::Ns(s) => s.len(),
        Value::Bs(s) => s.len(),
        Value::L(l) => l.len(),
        Value::M(m) => m.len(),
        Value::N(_) | Value::Bool(_) | Value::Null => return None,
    };
    i64::try_from(n).ok()
}

fn compare(op: Cmp, a: Option<&Value>, b: Option<&Value>) -> bool {
    let (Some(a), Some(b)) = (a, b) else {
        return op == Cmp::Ne;
    };
    match op {
        Cmp::Eq => a == b,
        Cmp::Ne => a != b,
        Cmp::Lt => a.compare(b).is_some_and(Ordering::is_lt),
        Cmp::Le => a.compare(b).is_some_and(Ordering::is_le),
        Cmp::Gt => a.compare(b).is_some_and(Ordering::is_gt),
        Cmp::Ge => a.compare(b).is_some_and(Ordering::is_ge),
    }
}

fn function(f: Func, args: &[Operand], item: &Item) -> bool {
    let (first, second) = (args.first(), args.get(1));
    let a = first.and_then(|o| operand(o, item));
    let b = second.and_then(|o| operand(o, item));
    match f {
        Func::AttributeExists => a.is_some(),
        Func::AttributeNotExists => a.is_none(),
        Func::AttributeType => match (a.as_deref(), b.as_deref()) {
            (Some(a), Some(Value::S(t))) => a.type_name() == t,
            _ => false,
        },
        Func::BeginsWith => match (a.as_deref(), b.as_deref()) {
            (Some(Value::S(s)), Some(Value::S(p))) => s.starts_with(p.as_str()),
            (Some(Value::B(s)), Some(Value::B(p))) => s.starts_with(p),
            _ => false,
        },
        Func::Contains => match (a.as_deref(), b.as_deref()) {
            (Some(a), Some(b)) => contains(a, b),
            _ => false,
        },
    }
}

fn contains(whole: &Value, part: &Value) -> bool {
    match (whole, part) {
        (Value::S(s), Value::S(p)) => s.contains(p.as_str()),
        (Value::B(s), Value::B(p)) => p.is_empty() || s.windows(p.len()).any(|w| w == p.as_slice()),
        (Value::Ss(s), Value::S(p)) => s.contains(p),
        (Value::Ns(s), Value::N(p)) => s.contains(p),
        (Value::Bs(s), Value::B(p)) => s.contains(p),
        (Value::L(l), p) => l.contains(p),
        _ => false,
    }
}

/// The attribute an operand reads, if it reads one.
fn root_of(o: &Operand) -> Option<&str> {
    match o {
        Operand::Path(p) | Operand::Size(p) | Operand::IfNotExists(p, _) => Some(p.root()),
        Operand::Value(_) | Operand::ListAppend(..) => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::ast::{NameMap, ValueMap};
    use crate::parse_cond::parse_condition;
    use crate::scope::{Kind, Scope};
    use crate::value::item_from_json;

    fn check(text: &str, values: serde_json::Value, item: serde_json::Value) -> bool {
        let values: ValueMap = values
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), Value::from_json(v).unwrap()))
            .collect();
        let names = NameMap::new();
        let cond =
            parse_condition(text, Kind::Condition, &mut Scope::new(&names, &values)).unwrap();
        cond.eval(&item_from_json(&item).unwrap())
    }

    fn item() -> serde_json::Value {
        json!({"pk": {"S": "a"}, "n": {"N": "5"}, "t": {"SS": ["x", "y"]}, "l": {"L": [{"S": "q"}]}, "m": {"M": {"k": {"N": "1"}}}})
    }

    #[test]
    fn comparisons_and_missing_attributes() {
        let v = json!({":five": {"N": "5.0"}, ":six": {"N": "6"}, ":s": {"S": "a"}});
        assert!(check(
            "n = :five AND n < :six AND n <> :six",
            v.clone(),
            item()
        ));
        assert!(!check("n > :six", v.clone(), item()));
        assert!(!check("pk = :five", v.clone(), item()));
        assert!(check("pk <> :five", v.clone(), item()));
        assert!(!check("absent = :s", v.clone(), item()));
        assert!(check("absent <> :s", v.clone(), item()));
        assert!(check("NOT absent = :s", v, item()));
    }

    #[test]
    fn functions() {
        let v = json!({":a": {"S": "a"}, ":x": {"S": "x"}, ":t": {"S": "SS"}, ":one": {"N": "1"}, ":q": {"S": "q"}});
        assert!(check(
            "attribute_exists(pk) AND attribute_not_exists(zz)",
            v.clone(),
            item()
        ));
        assert!(check(
            "begins_with(pk, :a) AND contains(t, :x) AND contains(l, :q)",
            v.clone(),
            item()
        ));
        assert!(check(
            "attribute_type(t, :t) AND size(l) = :one AND size(t) > :one",
            v.clone(),
            item()
        ));
        assert!(check("m.k = :one", v.clone(), item()));
        assert!(!check("size(n) > :one", v, item()));
    }

    #[test]
    fn between_and_in() {
        let v = json!({":lo": {"N": "1"}, ":hi": {"N": "5"}, ":a": {"S": "a"}, ":b": {"S": "b"}});
        assert!(check("n BETWEEN :lo AND :hi", v.clone(), item()));
        assert!(check("pk IN (:a, :b)", v.clone(), item()));
        assert!(!check("pk IN (:b)", v, item()));
    }
}
