//! The metric math functions, as AWS's "Metric math syntax and functions" page lists them (memory: their names and what each takes
//! and returns are from that page, not recorded). Built: `ABS CEIL FLOOR LOG LOG10`, `AVG SUM MIN MAX STDDEV` (over a series: a
//! scalar; over an array: a series, per timestamp), `DATAPOINT_COUNT`, `PERIOD`, `TIME_SERIES`, `IF`, `METRICS`, `REMOVE_EMPTY`,
//! `FIRST`, `LAST`, `SORT`, `SLICE`, `SEARCH`, and in [`super::series_fns`] `FILL RATE DIFF DIFF_TIME RUNNING_SUM
//! ANOMALY_DETECTION_BAND` and the time functions. Functions that read other services (`INSIGHT_RULE_METRIC`, `SERVICE_QUOTA`,
//! `LAMBDA`, `DB_PERF_INSIGHTS`) are refused with a sentence.

use std::collections::BTreeMap;

use super::eval::Env;
use super::series_fns as sf;
use super::value::{Ts, Val};

/// The most series a `SEARCH` returns (memory: AWS's limit is 500).
pub(crate) const MAX_SEARCH: usize = 500;

/// Calls `f` on `args`.
///
/// # Errors
/// What is wrong, in a sentence.
pub(crate) fn call(f: &str, args: Vec<Val>, env: &Env<'_>) -> Result<Val, String> {
    match f {
        "ABS" => one(f, args)?.map(&f64::abs),
        "CEIL" => one(f, args)?.map(&f64::ceil),
        "FLOOR" => one(f, args)?.map(&f64::floor),
        "LOG" => one(f, args)?.map(&f64::ln),
        "LOG10" => one(f, args)?.map(&f64::log10),
        "AVG" | "SUM" | "MIN" | "MAX" | "STDDEV" => aggregate(f, one(f, args)?),
        "DATAPOINT_COUNT" => count(one(f, args)?),
        "PERIOD" => match one(f, args)? {
            Val::Series(_) | Val::Array(_) => Ok(Val::Scalar(seconds(env.period_ms))),
            other => Err(format!("PERIOD takes a time series, not {}", other.kind())),
        },
        "TIME_SERIES" => match one(f, args)? {
            Val::Scalar(x) => Ok(Val::Series(Ts::new(
                "",
                env.grid.iter().map(|t| (*t, x)).collect(),
            ))),
            other => Err(format!("TIME_SERIES takes a scalar, not {}", other.kind())),
        },
        "IF" => if_(args),
        "METRICS" => metrics(args, env),
        "REMOVE_EMPTY" => Ok(Val::Array(
            array(f, one(f, args)?)?
                .into_iter()
                .filter(|t| !t.points.is_empty())
                .collect(),
        )),
        "FIRST" => Ok(array(f, one(f, args)?)?
            .into_iter()
            .next()
            .map_or(Val::Series(Ts::default()), Val::Series)),
        "LAST" => Ok(array(f, one(f, args)?)?
            .pop()
            .map_or(Val::Series(Ts::default()), Val::Series)),
        "SORT" => sort(args),
        "SLICE" => slice(args),
        "SEARCH" => search(args, env),
        _ => sf::call(f, args, env),
    }
}

pub(crate) fn seconds(ms: i64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let s = ms as f64 / 1000.0;
    s
}

/// The single argument of `f`.
pub(crate) fn one(f: &str, args: Vec<Val>) -> Result<Val, String> {
    let n = args.len();
    let mut it = args.into_iter();
    match (it.next(), n) {
        (Some(v), 1) => Ok(v),
        _ => Err(format!("{f} takes one argument, not {n}")),
    }
}

/// An argument that must be an array (a single series counts as an array of one).
pub(crate) fn array(f: &str, v: Val) -> Result<Vec<Ts>, String> {
    match v {
        Val::Array(all) => Ok(all),
        Val::Series(ts) => Ok(vec![ts]),
        other => Err(format!(
            "{f} takes an array of time series, not {}",
            other.kind()
        )),
    }
}

/// `AVG`, `SUM`, `MIN`, `MAX` or `STDDEV` of numbers; `None` when there are none.
pub(crate) fn of(f: &str, xs: &[f64]) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    let n = xs.len() as f64;
    let sum: f64 = xs.iter().sum();
    Some(match f {
        "SUM" => sum,
        "MIN" => xs.iter().copied().fold(f64::INFINITY, f64::min),
        "MAX" => xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        "COUNT" => n,
        // The population standard deviation (memory: AWS does not say which).
        "STDDEV" => (xs.iter().map(|x| (x - sum / n).powi(2)).sum::<f64>() / n).sqrt(),
        _ => sum / n,
    })
}

fn aggregate(f: &str, v: Val) -> Result<Val, String> {
    match v {
        Val::Series(ts) => Ok(Val::Scalar(
            of(f, &ts.points.values().copied().collect::<Vec<_>>()).unwrap_or(f64::NAN),
        )),
        Val::Array(all) => {
            let mut at: BTreeMap<i64, Vec<f64>> = BTreeMap::new();
            for ts in &all {
                for (t, x) in &ts.points {
                    at.entry(*t).or_default().push(*x);
                }
            }
            Ok(Val::Series(Ts::new(
                "",
                at.iter()
                    .filter_map(|(t, xs)| Some((*t, of(f, xs)?)))
                    .collect(),
            )))
        }
        Val::Scalar(x) => Ok(Val::Scalar(x)),
        other => Err(format!(
            "{f} takes a time series or an array, not {}",
            other.kind()
        )),
    }
}

fn count(v: Val) -> Result<Val, String> {
    match v {
        #[allow(clippy::cast_precision_loss)]
        Val::Series(ts) => Ok(Val::Scalar(ts.points.len() as f64)),
        Val::Array(all) => {
            let mut at: BTreeMap<i64, f64> = BTreeMap::new();
            for ts in &all {
                for t in ts.points.keys() {
                    *at.entry(*t).or_default() += 1.0;
                }
            }
            Ok(Val::Series(Ts::new("", at)))
        }
        other => Err(format!(
            "DATAPOINT_COUNT takes a time series, not {}",
            other.kind()
        )),
    }
}

/// The value of `v` at `t`: a scalar everywhere, a series where it has a datapoint.
fn at(v: &Val, t: i64) -> Option<f64> {
    match v {
        Val::Scalar(x) => Some(*x),
        Val::Series(ts) => ts.points.get(&t).copied(),
        _ => None,
    }
}

/// `IF(condition, then [, else])`: where the condition is not 0, `then`; elsewhere `else`, or no datapoint when there is none.
fn if_(args: Vec<Val>) -> Result<Val, String> {
    let n = args.len();
    let mut it = args.into_iter();
    let (Some(cond), Some(then), other, true) =
        (it.next(), it.next(), it.next(), (2..=3).contains(&n))
    else {
        return Err(format!("IF takes two or three arguments, not {n}"));
    };
    for v in std::iter::once(&then).chain(other.as_ref()) {
        if !matches!(v, Val::Scalar(_) | Val::Series(_)) {
            return Err(format!(
                "IF takes a scalar or a time series, not {}",
                v.kind()
            ));
        }
    }
    match cond {
        Val::Scalar(c) if c != 0.0 => Ok(then),
        Val::Scalar(_) => Ok(other.unwrap_or_else(|| Val::Series(Ts::default()))),
        Val::Series(c) => Ok(Val::Series(
            c.with(
                c.points
                    .iter()
                    .filter_map(|(t, x)| {
                        let v = if *x != 0.0 {
                            at(&then, *t)
                        } else {
                            at(other.as_ref()?, *t)
                        };
                        Some((*t, v?))
                    })
                    .collect(),
            ),
        )),
        other => Err(format!(
            "IF's condition is a scalar or a time series, not {}",
            other.kind()
        )),
    }
}

/// `METRICS()`: every metric of the request; `METRICS('x')`: those whose id holds `x` (memory).
fn metrics(args: Vec<Val>, env: &Env<'_>) -> Result<Val, String> {
    let filter = match args.as_slice() {
        [] => None,
        [Val::Text(s)] => Some(s.clone()),
        _ => return Err("METRICS takes nothing, or one string".to_owned()),
    };
    Ok(Val::Array(
        env.src
            .metrics()
            .into_iter()
            .filter(|(id, _)| filter.as_deref().is_none_or(|f| id.contains(f)))
            .map(|(_, ts)| ts)
            .collect(),
    ))
}

/// `SORT(array, AVG|COUNT|MAX|MIN|STDDEV|SUM, ASC|DESC [, n])`.
fn sort(args: Vec<Val>) -> Result<Val, String> {
    let mut it = args.into_iter();
    let (Some(arr), Some(Val::Word(by)), Some(Val::Word(order)), limit, None) =
        (it.next(), it.next(), it.next(), it.next(), it.next())
    else {
        return Err(
            "SORT takes an array, a function, ASC or DESC, and a number of results".to_owned(),
        );
    };
    if !["AVG", "COUNT", "MAX", "MIN", "STDDEV", "SUM"].contains(&by.as_str())
        || !["ASC", "DESC"].contains(&order.as_str())
    {
        return Err(format!("SORT cannot sort by {by} {order}"));
    }
    let mut all: Vec<(f64, Ts)> = array("SORT", arr)?
        .into_iter()
        .map(|ts| {
            (
                of(&by, &ts.points.values().copied().collect::<Vec<_>>()).unwrap_or(f64::NAN),
                ts,
            )
        })
        .collect();
    all.sort_by(|a, b| a.0.total_cmp(&b.0));
    if order == "DESC" {
        all.reverse();
    }
    let keep = match limit {
        None => usize::MAX,
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Some(Val::Scalar(n)) if n >= 0.0 => n as usize,
        Some(other) => {
            return Err(format!(
                "SORT's number of results is a number, not {}",
                other.kind()
            ));
        }
    };
    Ok(Val::Array(
        all.into_iter().take(keep).map(|(_, ts)| ts).collect(),
    ))
}

/// `SLICE(array, from, to)`: the series from index `from` up to `to` (memory).
fn slice(args: Vec<Val>) -> Result<Val, String> {
    let mut it = args.into_iter();
    let (Some(arr), Some(Val::Scalar(from)), Some(Val::Scalar(to)), None) =
        (it.next(), it.next(), it.next(), it.next())
    else {
        return Err("SLICE takes an array and two numbers".to_owned());
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (from, to) = (from.max(0.0) as usize, to.max(0.0) as usize);
    Ok(Val::Array(
        array("SLICE", arr)?
            .into_iter()
            .skip(from)
            .take(to.saturating_sub(from))
            .collect(),
    ))
}

/// `SEARCH('search expression', 'Statistic', period)`.
fn search(args: Vec<Val>, env: &Env<'_>) -> Result<Val, String> {
    let mut it = args.into_iter();
    let (Some(Val::Text(q)), Some(Val::Text(stat)), period, None) =
        (it.next(), it.next(), it.next(), it.next())
    else {
        return Err("SEARCH takes a search expression, a statistic and a period".to_owned());
    };
    #[allow(clippy::cast_possible_truncation)]
    let period_s = match period {
        None => env.period_ms / 1000,
        Some(Val::Scalar(p)) => p as i64,
        Some(other) => return Err(format!("SEARCH's period is a number, not {}", other.kind())),
    };
    let query = super::search::parse(&q)?;
    Ok(Val::Array(env.src.search(&query, &stat, period_s)?))
}
