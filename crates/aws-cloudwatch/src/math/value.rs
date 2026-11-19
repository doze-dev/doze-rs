//! What metric math works on: a number (a scalar), a time series, or an array of time series, and the operators between them. As
//! AWS's syntax page describes them (memory): an operator between a scalar and a series applies to every datapoint; between two
//! series it pairs the datapoints with the same timestamp, and a timestamp only one of them has gives nothing; between an array and
//! a scalar or a series it applies to each series of the array. Two arrays cannot be combined. A comparison or a logical operator
//! gives 1 for true and 0 for false. A result that is not a finite number (a division by zero) is no datapoint.

use std::collections::BTreeMap;

use super::lex::Op;

/// A time series: its label, and its datapoints by timestamp (ms).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Ts {
    pub(crate) label: String,
    pub(crate) points: BTreeMap<i64, f64>,
}

impl Ts {
    pub(crate) fn new(label: impl Into<String>, points: BTreeMap<i64, f64>) -> Self {
        Self {
            label: label.into(),
            points: points.into_iter().filter(|(_, v)| v.is_finite()).collect(),
        }
    }

    /// The same label, other points.
    pub(crate) fn with(&self, points: BTreeMap<i64, f64>) -> Self {
        Self::new(self.label.clone(), points)
    }

    fn map(&self, f: &dyn Fn(f64) -> f64) -> Self {
        self.with(self.points.iter().map(|(t, v)| (*t, f(*v))).collect())
    }
}

/// A value of an expression.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Val {
    Scalar(f64),
    Series(Ts),
    Array(Vec<Ts>),
    Text(String),
    /// A bare word: `REPEAT`, `LINEAR`, `ASC`, `DESC`, `SUM`...
    Word(String),
}

impl Val {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Scalar(_) => "a scalar",
            Self::Series(_) => "a time series",
            Self::Array(_) => "an array of time series",
            Self::Text(_) => "a string",
            Self::Word(_) => "a keyword",
        }
    }

    /// Applies `f` to every number the value holds.
    pub(crate) fn map(self, f: &dyn Fn(f64) -> f64) -> Result<Self, String> {
        Ok(match self {
            Self::Scalar(x) => Self::Scalar(f(x)),
            Self::Series(ts) => Self::Series(ts.map(f)),
            Self::Array(all) => Self::Array(all.iter().map(|ts| ts.map(f)).collect()),
            other => return Err(format!("expected numbers, found {}", other.kind())),
        })
    }
}

fn truth(x: f64) -> bool {
    x != 0.0
}

fn bool_num(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

/// `a op b` on two numbers.
pub(crate) fn apply(op: Op, a: f64, b: f64) -> f64 {
    match op {
        Op::Add => a + b,
        Op::Sub => a - b,
        Op::Mul => a * b,
        Op::Div => a / b,
        Op::Pow => a.powf(b),
        Op::Lt => bool_num(a < b),
        Op::Gt => bool_num(a > b),
        Op::Le => bool_num(a <= b),
        Op::Ge => bool_num(a >= b),
        #[allow(clippy::float_cmp)]
        Op::Eq => bool_num(a == b),
        #[allow(clippy::float_cmp)]
        Op::Ne => bool_num(a != b),
        Op::And => bool_num(truth(a) && truth(b)),
        Op::Or => bool_num(truth(a) || truth(b)),
        Op::Not => bool_num(!truth(b)),
    }
}

fn pair(op: Op, a: &Ts, b: &Ts) -> Ts {
    a.with(
        a.points
            .iter()
            .filter_map(|(t, x)| Some((*t, apply(op, *x, *b.points.get(t)?))))
            .collect(),
    )
}

/// `a op b` on two values.
pub(crate) fn combine(op: Op, a: Val, b: Val) -> Result<Val, String> {
    use Val::{Array, Scalar, Series};
    Ok(match (a, b) {
        (Scalar(x), Scalar(y)) => Scalar(apply(op, x, y)),
        (Series(ts), Scalar(y)) => Series(ts.map(&|x| apply(op, x, y))),
        (Scalar(x), Series(ts)) => Series(ts.map(&|y| apply(op, x, y))),
        (Series(a), Series(b)) => Series(pair(op, &a, &b)),
        (Array(all), Scalar(y)) => {
            Array(all.iter().map(|ts| ts.map(&|x| apply(op, x, y))).collect())
        }
        (Scalar(x), Array(all)) => {
            Array(all.iter().map(|ts| ts.map(&|y| apply(op, x, y))).collect())
        }
        (Array(all), Series(b)) => Array(all.iter().map(|a| pair(op, a, &b)).collect()),
        (Series(a), Array(all)) => Array(
            all.iter()
                .map(|b| {
                    let mut out = pair(op, &a, b);
                    out.label.clone_from(&b.label);
                    out
                })
                .collect(),
        ),
        (Array(_), Array(_)) => {
            return Err("two arrays of time series cannot be combined".to_owned());
        }
        (a, b) => {
            return Err(format!(
                "an operator cannot take {} and {}",
                a.kind(),
                b.kind()
            ));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(points: &[(i64, f64)]) -> Ts {
        Ts::new("x", points.iter().copied().collect())
    }

    #[test]
    fn series_pair_by_timestamp_and_a_division_by_zero_is_no_datapoint() {
        let a = ts(&[(1, 10.0), (2, 20.0), (3, 30.0)]);
        let b = ts(&[(1, 2.0), (3, 0.0)]);
        let Val::Series(out) = combine(Op::Div, Val::Series(a.clone()), Val::Series(b)).unwrap()
        else {
            panic!()
        };
        assert_eq!(out.points.into_iter().collect::<Vec<_>>(), [(1, 5.0)]);
        let Val::Series(out) = combine(Op::Gt, Val::Series(a), Val::Scalar(15.0)).unwrap() else {
            panic!()
        };
        assert_eq!(
            out.points.values().copied().collect::<Vec<_>>(),
            [0.0, 1.0, 1.0]
        );
        assert!(combine(Op::Add, Val::Array(vec![]), Val::Array(vec![])).is_err());
        assert!(combine(Op::Add, Val::Text("a".into()), Val::Scalar(1.0)).is_err());
    }
}
