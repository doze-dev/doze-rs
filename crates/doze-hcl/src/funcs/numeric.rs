//! cty's number functions (`stdlib/number.go`): abs, ceil, floor, max, min, parseint.

use super::{Spec, arg, str_arg, to_int};
use crate::function::{FuncError, Param};
use crate::number::Number;
use crate::types::Type;
use crate::value::Value;
use num_bigint::BigUint;
use std::cmp::Ordering;

pub(super) fn functions() -> Vec<(&'static str, Spec)> {
    let num = || vec![Param::new("num", Type::Number)];
    let numbers = Param::new("numbers", Type::Number).dynamic();
    vec![
        (
            "abs",
            Spec::new(
                vec![Param::new("num", Type::Number).dynamic()],
                Type::Number,
                abs,
            ),
        ),
        ("ceil", Spec::new(num(), Type::Number, ceil)),
        ("floor", Spec::new(num(), Type::Number, floor)),
        (
            "max",
            Spec::new(vec![], Type::Number, max).var(numbers.clone()),
        ),
        ("min", Spec::new(vec![], Type::Number, min).var(numbers)),
        (
            "parseint",
            Spec::rule(
                vec![
                    Param::new("number", Type::Dynamic),
                    Param::new("base", Type::Number),
                ],
                parseint_type,
                parseint,
            ),
        ),
    ]
}

type Out = Result<Value, FuncError>;

fn number(args: &[Value]) -> Number {
    arg(args, 0)
        .as_number()
        .cloned()
        .unwrap_or_else(|| Number::from_i64(0))
}

fn abs(args: &[Value], _: &Type) -> Out {
    Ok(Value::Number(number(args).abs()))
}

/// Toward positive (`up`) or negative infinity, at the argument's precision; an infinity is itself.
fn round(args: &[Value], up: bool) -> Out {
    let n = number(args);
    let Some((neg, mut mag)) = n.trunc() else {
        return Ok(Value::Number(n));
    };
    if !n.is_integer() {
        // Truncation went toward zero: away from the wanted direction when the sign agrees.
        let (neg_out, add) = match (neg, up) {
            (false, true) => (false, true),
            (true, false) => (true, true),
            _ => (neg, false),
        };
        if add {
            mag += 1u8;
        }
        return Ok(Value::Number(n.with_integer(neg_out, mag)));
    }
    Ok(Value::Number(n.with_integer(neg, mag)))
}

fn ceil(args: &[Value], _: &Type) -> Out {
    round(args, true)
}

fn floor(args: &[Value], _: &Type) -> Out {
    round(args, false)
}

/// The first number the rest do not beat (starting from the opposite infinity, as cty does).
fn extreme(args: &[Value], better: Ordering) -> Out {
    if args.is_empty() {
        return Err(FuncError::new("must pass at least one number"));
    }
    let start = Number::from_f64(if better == Ordering::Less {
        f64::INFINITY
    } else {
        f64::NEG_INFINITY
    })
    .unwrap_or_else(|| Number::from_i64(0));
    let mut best = Value::Number(start);
    for a in args {
        if let (Some(n), Some(b)) = (a.as_number(), best.as_number())
            && n.cmp_value(b) == better
        {
            best = a.clone();
        }
    }
    Ok(best)
}

fn max(args: &[Value], _: &Type) -> Out {
    extreme(args, Ordering::Greater)
}

fn min(args: &[Value], _: &Type) -> Out {
    extreme(args, Ordering::Less)
}

fn parseint_type(args: &[Value]) -> Result<Type, FuncError> {
    let t = arg(args, 0).ty();
    if t != Type::String {
        return Err(FuncError::arg(
            0,
            format!("first argument must be a string, not {}", t.friendly_name()),
        ));
    }
    Ok(Type::Number)
}

/// Go's `big.Int.SetString(s, base)`: an optional sign, then digits of the base (letters are
/// case-blind up to base 36; above it `a`–`z` are 10–35 and `A`–`Z` 36–61), nothing else.
fn parseint(args: &[Value], _: &Type) -> Out {
    let s = str_arg(args, 0);
    let base = to_int(arg(args, 1)).map_err(|e| FuncError::arg(1, e))?;
    let Some(base) = u32::try_from(base).ok().filter(|b| (2..=62).contains(b)) else {
        return Err(FuncError::arg(
            1,
            "base must be a whole number between 2 and 62 inclusive",
        ));
    };
    let bad = || {
        FuncError::arg(
            0,
            format!(
                "cannot parse {} as a base {base} integer",
                crate::value::quote::go_quote(s)
            ),
        )
    };
    let (neg, digits) = match s.as_bytes().first() {
        Some(b'-') => (true, s.get(1..).unwrap_or("")),
        Some(b'+') => (false, s.get(1..).unwrap_or("")),
        _ => (false, s),
    };
    if digits.is_empty() {
        return Err(bad());
    }
    let mut mag = BigUint::default();
    for c in digits.chars() {
        let d = match c {
            '0'..='9' => u32::from(c) - u32::from('0'),
            'a'..='z' => u32::from(c) - u32::from('a') + 10,
            'A'..='Z' if base <= 36 => u32::from(c) - u32::from('A') + 10,
            'A'..='Z' => u32::from(c) - u32::from('A') + 36,
            _ => return Err(bad()),
        };
        if d >= base {
            return Err(bad());
        }
        mag = mag * base + d;
    }
    Ok(Value::Number(Number::from_bigint(neg, mag)))
}
