//! Operators over unknown operands, with cty's refinements: the result is known not to be null, a
//! comparison is decided when the operands' ranges do not overlap, and `+`, `-` and `*` carry a
//! range when their operands have one (cty's `numericRangeArithmetic`).

use crate::number::{ArithError, Number};
use crate::types::Type;
use crate::value::{Refinement, Value};
use hcl_edit::expr::BinaryOperator as B;
use std::cmp::Ordering;

fn inf(neg: bool) -> Number {
    Number::parse(if neg { "-Inf" } else { "+Inf" }).unwrap_or_else(|| Number::from_i64(0))
}

/// The lower and upper bound of a number value (infinities where nothing is known).
fn bounds(v: &Value) -> (Number, Number) {
    let r = v.refinement();
    let lo = r.lower.map_or_else(|| inf(true), |(n, _)| n);
    let hi = r.upper.map_or_else(|| inf(false), |(n, _)| n);
    (lo, hi)
}

/// `a < b` (or `a > b` with `greater`) where either side may be unknown.
fn compare(a: &Value, b: &Value, greater: bool) -> Value {
    let (alo, ahi) = bounds(a);
    let (blo, bhi) = bounds(b);
    let (first, second) = if greater { (&alo, &bhi) } else { (&ahi, &blo) };
    let want = if greater {
        Ordering::Greater
    } else {
        Ordering::Less
    };
    if first.cmp_value(second) == want {
        return Value::Bool(true);
    }
    let (first, second) = if greater { (&ahi, &blo) } else { (&alo, &bhi) };
    if first.cmp_value(second) == want.reverse() {
        return Value::Bool(false);
    }
    Value::unknown_not_null(Type::Bool)
}

/// cty's `Value.Or` when one side may be unknown: a known `true` decides.
fn or(a: Value, b: Value) -> Value {
    match (a, b) {
        (Value::Bool(x), Value::Bool(y)) => Value::Bool(x || y),
        (Value::Bool(true), _) | (_, Value::Bool(true)) => Value::Bool(true),
        _ => Value::unknown_not_null(Type::Bool),
    }
}

/// The result of `op` when at least one operand is unknown (neither is null).
pub(super) fn unknown_operands(op: B, a: &Value, b: &Value) -> Value {
    match op {
        B::Less => compare(a, b, false),
        B::Greater => compare(a, b, true),
        B::LessEq => or(compare(a, b, false), a.equals(b)),
        B::GreaterEq => or(compare(a, b, true), a.equals(b)),
        B::Plus => ranged(a, b, Number::add),
        B::Minus => ranged(a, b, Number::sub),
        B::Mul => ranged(a, b, Number::mul),
        B::Div | B::Mod => Value::unknown_not_null(Type::Number),
        B::And | B::Or | B::Eq | B::NotEq => Value::unknown_not_null(Type::Bool),
    }
}

/// An unknown number whose range is `op` over the corners of the operands' ranges.
fn ranged(a: &Value, b: &Value, op: fn(&Number, &Number) -> Result<Number, ArithError>) -> Value {
    let (alo, ahi) = bounds(a);
    let (blo, bhi) = bounds(b);
    let corners: Option<Vec<Number>> = [(&alo, &blo), (&alo, &bhi), (&ahi, &blo), (&ahi, &bhi)]
        .into_iter()
        .map(|(x, y)| op(x, y).ok())
        .collect();
    let mut r = Refinement::not_null();
    if let Some(cs) = corners {
        let min = cs.iter().min_by(|x, y| x.cmp_value(y)).cloned();
        let max = cs.iter().max_by(|x, y| x.cmp_value(y)).cloned();
        r.lower = min
            .filter(|m| !(m.is_inf() && m.is_negative()))
            .map(|m| (m, true));
        r.upper = max
            .filter(|m| !(m.is_inf() && !m.is_negative()))
            .map(|m| (m, true));
    }
    Value::refined(Type::Number, r)
}
