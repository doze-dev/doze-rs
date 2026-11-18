//! Evaluating an expression against a row.

use super::ast::{Expr, Op, Period};
use super::record::Row;
use super::value::{Val, compare, equal};

/// The value of `e` for `row`; `slots` holds the aggregates of the row's group, after a stats command.
pub(crate) fn eval(e: &Expr, row: &Row, slots: &[Val]) -> Val {
    match e {
        Expr::Lit(v) => v.clone(),
        Expr::Field(f) => row.get(f).cloned().unwrap_or(Val::Null),
        Expr::Regex(_) | Expr::List(_) | Expr::Star | Expr::Agg(..) => Val::Null,
        #[allow(clippy::cast_precision_loss)]
        Expr::Period(p) => match p {
            Period::Ms(ms) => Val::Num(*ms as f64),
            Period::Months(m) => Val::Num(*m as f64 * 30.0 * 86_400_000.0),
        },
        Expr::Slot(i) => slots.get(*i).cloned().unwrap_or(Val::Null),
        Expr::Neg(x) => eval(x, row, slots)
            .num()
            .map_or(Val::Null, |n| Val::Num(-n)),
        Expr::Not(x) => Val::Bool(!eval(x, row, slots).truthy()),
        Expr::Bin(op, a, b) => binary(*op, a, b, row, slots),
        Expr::Call(name, args) => super::funcs::call(name, args, row, slots),
    }
}

fn binary(op: Op, a: &Expr, b: &Expr, row: &Row, slots: &[Val]) -> Val {
    match op {
        Op::Or => Val::Bool(eval(a, row, slots).truthy() || eval(b, row, slots).truthy()),
        Op::And => Val::Bool(eval(a, row, slots).truthy() && eval(b, row, slots).truthy()),
        Op::Like => Val::Bool(like(&eval(a, row, slots), b, row, slots)),
        Op::NotLike => {
            let v = eval(a, row, slots);
            Val::Bool(!v.is_null() && !like(&v, b, row, slots))
        }
        Op::In => Val::Bool(member(&eval(a, row, slots), b, row, slots)),
        Op::NotIn => {
            let v = eval(a, row, slots);
            Val::Bool(!v.is_null() && !member(&v, b, row, slots))
        }
        _ => {
            let (x, y) = (eval(a, row, slots), eval(b, row, slots));
            compared(op, &x, &y).unwrap_or_else(|| arithmetic(op, &x, &y))
        }
    }
}

/// A comparison's answer, or `None` when `op` is not a comparison.
fn compared(op: Op, x: &Val, y: &Val) -> Option<Val> {
    let present = !x.is_null() && !y.is_null();
    let ord = compare(x, y);
    Some(Val::Bool(match op {
        Op::Eq => equal(x, y),
        Op::Ne => present && !equal(x, y),
        Op::Lt => present && ord.is_lt(),
        Op::Le => present && ord.is_le(),
        Op::Gt => present && ord.is_gt(),
        Op::Ge => present && ord.is_ge(),
        _ => return None,
    }))
}

fn arithmetic(op: Op, x: &Val, y: &Val) -> Val {
    let (Some(a), Some(b)) = (x.num(), y.num()) else {
        return Val::Null;
    };
    let n = match op {
        Op::Add => a + b,
        Op::Sub => a - b,
        Op::Mul => a * b,
        Op::Div if b != 0.0 => a / b,
        Op::Mod if b != 0.0 => a % b,
        Op::Pow => a.powf(b),
        _ => return Val::Null,
    };
    if n.is_finite() {
        Val::Num(n)
    } else {
        Val::Null
    }
}

/// `like` and `=~`: a regular expression matches anywhere; a string is looked for as a part of the value. (memory)
fn like(v: &Val, pattern: &Expr, row: &Row, slots: &[Val]) -> bool {
    if v.is_null() {
        return false;
    }
    let text = v.text();
    match pattern {
        Expr::Regex(re) => re.is_match(&text),
        other => {
            let needle = eval(other, row, slots).text();
            text.contains(&needle)
        }
    }
}

/// `in [a, b, ...]`.
fn member(v: &Val, list: &Expr, row: &Row, slots: &[Val]) -> bool {
    match list {
        Expr::List(items) => items.iter().any(|i| equal(v, &eval(i, row, slots))),
        other => equal(v, &eval(other, row, slots)),
    }
}
