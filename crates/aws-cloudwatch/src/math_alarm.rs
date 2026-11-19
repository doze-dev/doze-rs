//! A metric math alarm (`PutMetricAlarm` with `Metrics`): its queries checked when it is put, and its expression judged as a metric
//! alarm's statistic is. The rules are the model's documentation of `MetricDataQuery` and `PutMetricAlarm`: at most 20 queries, 10
//! of them `MetricStat` and 10 `Expression`, exactly one with `ReturnData` true, whose value is a single time series; the
//! `Metrics` exclude `Namespace`, `MetricName`, `Dimensions`, `Period`, `Unit`, `Statistic` and `ExtendedStatistic`; an anomaly
//! alarm names its `ANOMALY_DETECTION_BAND` expression in `ThresholdMetricId`. The words of each refusal are (memory), and so are
//! two rules the model does not state: every metric of the alarm has the same period, and `SEARCH` is refused in an alarm.
//!
//! An expression that looks along time (`RATE`, `DIFF`, `DIFF_TIME`, `FILL`, `ANOMALY_DETECTION_BAND`) is worked out over the
//! window and up to [`LOOKBACK`] periods before it, so its first period has a datapoint before it and the band has data to stand on.

use serde_json::Value;

use crate::eval::{Plan, Steps, periods};
use crate::judge::{Bands, Verdict, judge};
use crate::math::{self, Body, Data, Query, Search, Stat, Ts, Val};
use crate::model::{Alarm, Spec};

/// Queries one alarm may have, and of each kind (the model).
const MAX_QUERIES: usize = 20;
const MAX_OF_A_KIND: usize = 10;
/// Periods before the window that an expression looking along time reads.
pub(crate) const LOOKBACK: i64 = 50;
/// The functions that read along time.
const ALONG_TIME: [&str; 5] = [
    "RATE",
    "DIFF",
    "DIFF_TIME",
    "FILL",
    "ANOMALY_DETECTION_BAND",
];
/// Members a math alarm cannot have (the model's `Metrics` documentation).
const EXCLUDED: [&str; 7] = [
    "Namespace",
    "MetricName",
    "Dimensions",
    "Period",
    "Unit",
    "Statistic",
    "ExtendedStatistic",
];
/// The comparisons that need a band.
const BAND_COMPARISONS: [&str; 3] = [
    "LessThanLowerOrGreaterThanUpperThreshold",
    "LessThanLowerThreshold",
    "GreaterThanUpperThreshold",
];

/// A math alarm's queries, and which of them it watches.
#[derive(Debug)]
pub(crate) struct MathAlarm {
    queries: Vec<Query>,
    watched: String,
    band: Option<String>,
    lookback: i64,
}

/// The plan of a stored math alarm (none when its request no longer reads, which a put would have refused).
pub(crate) fn plan(a: &Alarm) -> Option<Plan> {
    let req: Value = serde_json::from_str(&a.request).ok()?;
    let (spec, m) = read(&req).ok()?;
    let inputs = m
        .queries
        .iter()
        .filter_map(|q| match &q.body {
            Body::Stat(s) => Some(Spec {
                namespace: s.namespace.clone(),
                metric: s.metric.clone(),
                dims: s.dims.clone(),
                stat: s.stat.clone(),
                unit: s.unit.clone(),
                period_s: spec.period_s,
                evaluation_periods: spec.evaluation_periods + m.lookback,
                ..Spec::default()
            }),
            Body::Expr(_) => None,
        })
        .collect();
    Some(Plan {
        spec,
        inputs,
        math: Some(m),
    })
}

/// Reads and checks a math alarm's request: what it is judged by, and its queries. `Err` is the sentence of a `ValidationError`.
pub(crate) fn read(req: &Value) -> Result<(Spec, MathAlarm), String> {
    if let Some(k) = EXCLUDED.iter().find(|k| req.get(**k).is_some()) {
        // Recorded 2026-11-19 for Namespace; the same words for the other members are (memory).
        return Err(format!("{k} should not be set if list of Metrics is set."));
    }
    let raw = req
        .get("Metrics")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let queries = math::read(raw)?;
    let stats: Vec<&Stat> = queries
        .iter()
        .filter_map(|q| match &q.body {
            Body::Stat(s) => Some(s),
            Body::Expr(_) => None,
        })
        .collect();
    if queries.len() > MAX_QUERIES
        || stats.len() > MAX_OF_A_KIND
        || queries.len() - stats.len() > MAX_OF_A_KIND
    {
        return Err(format!(
            "An alarm can have at most {MAX_QUERIES} metrics: {MAX_OF_A_KIND} with a MetricStat and {MAX_OF_A_KIND} with an Expression."
        ));
    }
    let watched: Vec<&Query> = queries.iter().filter(|q| q.return_data).collect();
    let [watched] = watched.as_slice() else {
        return Err("Exactly one element of the metrics list should return data.".to_owned());
    };
    let watched = watched.id.clone();
    math::order(&queries)?;
    let exprs: Vec<(&str, &math::Expr)> = queries
        .iter()
        .filter_map(|q| match &q.body {
            Body::Expr(e) => Some((q.id.as_str(), e)),
            Body::Stat(_) => None,
        })
        .collect();
    if let Some((id, _)) = exprs.iter().find(|(_, e)| e.calls("SEARCH")) {
        return Err(format!(
            "Error in expression '{id}': SEARCH cannot be used in an alarm."
        ));
    }
    let period_s = stats.first().map_or(60, |s| s.period_s);
    if stats.iter().any(|s| s.period_s != period_s) {
        return Err("All metrics in an alarm must have the same period.".to_owned());
    }
    let spec = spec_of(req, period_s)?;
    let band = band_of(req, &spec, &exprs)?;
    let lookback = if exprs
        .iter()
        .any(|(_, e)| ALONG_TIME.iter().any(|f| e.calls(f)))
    {
        LOOKBACK
    } else {
        0
    };
    let m = MathAlarm {
        queries,
        watched,
        band,
        lookback,
    };
    m.dry_run(&spec)?;
    Ok((spec, m))
}

fn spec_of(req: &Value, period_s: i64) -> Result<Spec, String> {
    let n = req
        .get("EvaluationPeriods")
        .and_then(Value::as_i64)
        .ok_or("The parameter EvaluationPeriods is required.")?;
    let comparison = req
        .get("ComparisonOperator")
        .and_then(Value::as_str)
        .ok_or("The parameter ComparisonOperator is required.")?;
    Ok(Spec {
        period_s,
        evaluation_periods: n,
        datapoints_to_alarm: req
            .get("DatapointsToAlarm")
            .and_then(Value::as_i64)
            .unwrap_or(n),
        threshold: req.get("Threshold").and_then(Value::as_f64).unwrap_or(0.0),
        comparison: comparison.to_owned(),
        treat_missing: req
            .get("TreatMissingData")
            .and_then(Value::as_str)
            .unwrap_or("missing")
            .to_owned(),
        ..Spec::default()
    })
}

/// The id of the band an anomaly alarm compares with: named by `ThresholdMetricId`, an `ANOMALY_DETECTION_BAND` expression.
fn band_of(
    req: &Value,
    spec: &Spec,
    exprs: &[(&str, &math::Expr)],
) -> Result<Option<String>, String> {
    let banded = BAND_COMPARISONS.contains(&spec.comparison.as_str());
    match req.get("ThresholdMetricId").and_then(Value::as_str) {
        None if banded => Err(format!(
            "The comparison {} needs a ThresholdMetricId.",
            spec.comparison
        )),
        None => Ok(None),
        Some(_) if !banded => Err(format!(
            "ThresholdMetricId cannot be used with the comparison {}.",
            spec.comparison
        )),
        Some(id) => match exprs.iter().find(|(i, _)| *i == id) {
            Some((_, e)) if e.calls("ANOMALY_DETECTION_BAND") => Ok(Some(id.to_owned())),
            _ => Err(format!(
                "ThresholdMetricId {id} must be the id of an ANOMALY_DETECTION_BAND expression."
            )),
        },
    }
}

/// The data an alarm is judged on: each metric's value per period, read from the steps held in memory.
struct Held<'a>(&'a [(Stat, Ts)]);

impl Data for Held<'_> {
    fn read(&self, stat: &Stat) -> Result<Ts, String> {
        Ok(self
            .0
            .iter()
            .find(|(s, _)| s == stat)
            .map(|(_, ts)| ts.clone())
            .unwrap_or_default())
    }

    fn search(&self, _: &Search, _: &str, _: i64) -> Result<Vec<Ts>, String> {
        Err("SEARCH cannot be used in an alarm.".to_owned())
    }
}

impl MathAlarm {
    fn stats(&self) -> impl Iterator<Item = &Stat> {
        self.queries.iter().filter_map(|q| match &q.body {
            Body::Stat(s) => Some(s),
            Body::Expr(_) => None,
        })
    }

    /// Works the queries out on no data, so a watched expression that is not a single series is refused when the alarm is put.
    fn dry_run(&self, spec: &Spec) -> Result<(), String> {
        let grid = math::grid(0, spec.period_s * 1000, spec.period_s * 1000, 1);
        let values = math::solve(&self.queries, &grid, spec.period_s * 1000, &Held(&[]))?;
        match values.get(&self.watched) {
            Some(Val::Series(_)) => Ok(()),
            Some(other) => Err(format!(
                "Error in expression '{}': the expression an alarm watches must give a single time series, not {}.",
                self.watched,
                other.kind()
            )),
            None => Ok(()),
        }
    }

    /// The verdict on the window that ends at `end`, from each metric's data by step (in the plan's order).
    pub(crate) fn verdict(&self, plan: &Plan, steps: &[Steps], end: i64, now: i64) -> Verdict {
        let spec = &plan.spec;
        let period = spec.period_s * 1000;
        let total = spec.evaluation_periods + self.lookback;
        let grid: Vec<i64> = (0..total).rev().map(|k| end - (k + 1) * period).collect();
        let held: Vec<(Stat, Ts)> = self
            .stats()
            .zip(plan.inputs.iter().zip(steps))
            .map(|(s, (input, st))| (s.clone(), Ts::new("", periods(input, st, end))))
            .collect();
        let values = math::solve(&self.queries, &grid, period, &Held(&held)).unwrap_or_default();
        let from = end - spec.evaluation_periods * period;
        let window = match values.get(&self.watched) {
            Some(Val::Series(ts)) => ts.points.range(from..).map(|(t, v)| (*t, *v)).collect(),
            _ => crate::judge::Window::new(),
        };
        let bands = self.band.as_ref().map(|id| bands(values.get(id), from));
        judge(spec, &window, bands.as_ref(), end, now)
    }
}

/// The band's two edges by period: lower and upper, whichever order the expression gives them in.
fn bands(v: Option<&Val>, from: i64) -> Bands {
    let mut out = Bands::new();
    if let Some(Val::Array(edges)) = v {
        for ts in edges {
            for (t, x) in ts.points.range(from..) {
                let e = out.entry(*t).or_insert((*x, *x));
                *e = (e.0.min(*x), e.1.max(*x));
            }
        }
    }
    out
}
