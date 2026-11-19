//! The functions that read a series along time: `FILL`, `RATE`, `DIFF`, `DIFF_TIME`, `RUNNING_SUM`, `ANOMALY_DETECTION_BAND`, and the
//! time of each datapoint (`EPOCH MINUTE HOUR DAY DATE MONTH YEAR`). Each applies to every series of an array.
//!
//! `ANOMALY_DETECTION_BAND` departs from AWS on purpose: AWS trains a model on up to two weeks of the metric; here the band is the
//! mean of the series' datapoints in the window read, plus and minus the given number of standard deviations (2 when none is
//! given). It is a statistical stand-in that lets an anomaly alarm be built and tried locally, not AWS's model.

use std::collections::BTreeMap;

use super::eval::Env;
use super::funcs::{of, one, seconds};
use super::value::{Ts, Val};

/// Functions that need a service doze-aws does not consult for metric math.
const ELSEWHERE: &[&str] = &[
    "INSIGHT_RULE_METRIC",
    "SERVICE_QUOTA",
    "LAMBDA",
    "DB_PERF_INSIGHTS",
];

/// Calls `f` on `args`.
///
/// # Errors
/// What is wrong, in a sentence.
pub(crate) fn call(f: &str, args: Vec<Val>, env: &Env<'_>) -> Result<Val, String> {
    match f {
        "FILL" => fill(args, env),
        "RATE" => each(f, one(f, args)?, &|ts| {
            steps(ts, &|(_, a), (_, b), dt| (b - a) / dt)
        }),
        "DIFF" => each(f, one(f, args)?, &|ts| {
            steps(ts, &|(_, a), (_, b), _| b - a)
        }),
        "DIFF_TIME" => each(f, one(f, args)?, &|ts| steps(ts, &|_, _, dt| dt)),
        "RUNNING_SUM" => each(f, one(f, args)?, &running_sum),
        "ANOMALY_DETECTION_BAND" => band(args, env),
        "EPOCH" | "MINUTE" | "HOUR" | "DAY" | "DATE" | "MONTH" | "YEAR" => {
            each(f, one(f, args)?, &|ts| {
                ts.with(ts.points.keys().map(|t| (*t, time_part(f, *t))).collect())
            })
        }
        _ if ELSEWHERE.contains(&f) => {
            Err(format!("The function {f} is not supported in doze-aws."))
        }
        _ => Err(format!("Unknown function {f}")),
    }
}

fn each(f: &str, v: Val, g: &dyn Fn(&Ts) -> Ts) -> Result<Val, String> {
    match v {
        Val::Series(ts) => Ok(Val::Series(g(&ts))),
        Val::Array(all) => Ok(Val::Array(all.iter().map(g).collect())),
        other => Err(format!("{f} takes a time series, not {}", other.kind())),
    }
}

/// Each datapoint against the one before it: `g(previous, this, seconds between)`. The first datapoint has none before it.
fn steps(ts: &Ts, g: &dyn Fn((i64, f64), (i64, f64), f64) -> f64) -> Ts {
    let pts: Vec<(i64, f64)> = ts.points.iter().map(|(t, v)| (*t, *v)).collect();
    ts.with(
        pts.windows(2)
            .filter_map(|w| match w {
                [a, b] => Some((b.0, g(*a, *b, seconds(b.0 - a.0)))),
                _ => None,
            })
            .collect(),
    )
}

fn running_sum(ts: &Ts) -> Ts {
    let mut total = 0.0;
    ts.with(
        ts.points
            .iter()
            .map(|(t, v)| {
                total += v;
                (*t, total)
            })
            .collect(),
    )
}

/// `FILL(series, value | series | REPEAT | LINEAR)`: a value for every period of the window that has none. `REPEAT` takes the
/// datapoint before; `LINEAR` the line between the datapoints on either side. Periods before the first datapoint (and, for
/// `LINEAR`, after the last) stay empty (memory).
fn fill(args: Vec<Val>, env: &Env<'_>) -> Result<Val, String> {
    let mut it = args.into_iter();
    let (Some(x), Some(with), None) = (it.next(), it.next(), it.next()) else {
        return Err("FILL takes a time series and what to fill it with".to_owned());
    };
    if let Val::Word(w) = &with
        && w != "REPEAT"
        && w != "LINEAR"
    {
        return Err(format!("FILL cannot fill with {w}"));
    }
    each("FILL", x, &|ts| {
        let mut points = ts.points.clone();
        for t in env.grid {
            if points.contains_key(t) {
                continue;
            }
            if let Some(v) = filler(&ts.points, *t, &with) {
                points.insert(*t, v);
            }
        }
        ts.with(points)
    })
}

fn filler(points: &BTreeMap<i64, f64>, t: i64, with: &Val) -> Option<f64> {
    let before = points.range(..t).next_back();
    match with {
        Val::Scalar(x) => Some(*x),
        Val::Series(other) => other.points.get(&t).copied(),
        Val::Word(w) if w == "REPEAT" => before.map(|(_, v)| *v),
        Val::Word(_) => {
            let ((t0, v0), (t1, v1)) = (before?, points.range(t..).next()?);
            #[allow(clippy::cast_precision_loss)]
            let f = (t - t0) as f64 / (t1 - t0) as f64;
            Some(v0 + (v1 - v0) * f)
        }
        _ => None,
    }
}

/// `ANOMALY_DETECTION_BAND(series [, deviations])`: an array of the band's upper and lower edges, over every period of the window.
fn band(args: Vec<Val>, env: &Env<'_>) -> Result<Val, String> {
    let mut it = args.into_iter();
    let (Some(Val::Series(ts)), k, None) = (it.next(), it.next(), it.next()) else {
        return Err(
            "ANOMALY_DETECTION_BAND takes a time series and a number of standard deviations"
                .to_owned(),
        );
    };
    let k = match k {
        None => 2.0,
        Some(Val::Scalar(k)) => k,
        Some(other) => {
            return Err(format!(
                "ANOMALY_DETECTION_BAND's width is a number, not {}",
                other.kind()
            ));
        }
    };
    let xs: Vec<f64> = ts.points.values().copied().collect();
    let (Some(mean), Some(sd)) = (of("AVG", &xs), of("STDDEV", &xs)) else {
        return Ok(Val::Array(vec![
            Ts::new(format!("{} Upper", ts.label), BTreeMap::new()),
            Ts::new(format!("{} Lower", ts.label), BTreeMap::new()),
        ]));
    };
    let edge = |v: f64, name: &str| {
        Ts::new(
            format!("{} {name}", ts.label),
            env.grid.iter().map(|t| (*t, v)).collect(),
        )
    };
    Ok(Val::Array(vec![
        edge(mean + k * sd, "Upper"),
        edge(mean - k * sd, "Lower"),
    ]))
}

/// A part of the time `t_ms` (UTC): `DAY` is the day of the week, Monday 1 to Sunday 7 (memory).
fn time_part(f: &str, t_ms: i64) -> f64 {
    let secs = t_ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let (y, m, d) = civil(days);
    #[allow(clippy::cast_precision_loss)]
    let v = match f {
        "EPOCH" => secs,
        "MINUTE" => secs.div_euclid(60).rem_euclid(60),
        "HOUR" => secs.div_euclid(3600).rem_euclid(24),
        "DAY" => (days + 3).rem_euclid(7) + 1,
        "DATE" => d,
        "MONTH" => m,
        _ => y,
    } as f64;
    v
}

/// Year, month and day of a count of days since 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_time_of_a_datapoint_is_read_in_utc() {
        // 2023-11-14 22:13:00 UTC, a Tuesday.
        let t = 1_699_999_980_000;
        let parts: Vec<f64> = ["MINUTE", "HOUR", "DAY", "DATE", "MONTH", "YEAR"]
            .iter()
            .map(|f| time_part(f, t))
            .collect();
        assert_eq!(parts, [13.0, 22.0, 2.0, 14.0, 11.0, 2023.0]);
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(-1), (1969, 12, 31));
    }
}
