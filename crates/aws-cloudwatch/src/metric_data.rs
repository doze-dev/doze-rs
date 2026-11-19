//! `GetMetricData`: metrics read by `MetricStat`, and metric math over them (`math`). Each query with `ReturnData` (true unless it
//! says false) is answered: a series as one result, an array (`SEARCH`, `METRICS()`) as one result per series under the query's id,
//! a scalar as its value at each timestamp the request's metrics have (memory: how AWS shows a scalar is not recorded).

use std::collections::{BTreeMap, BTreeSet};

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::math::{self, Body, Data, Query, Search, Stat, Ts, Val};
use crate::model::{UNITS, series_key};
use crate::read::{align, check_period};
use crate::req::{Req, bad_value, secs, validation};
use crate::stats::{Agg, is_percentile, known, value_of};

/// The most queries one call may hold (the model: 500).
const MAX_QUERIES: usize = 500;
/// The most datapoints one call reads per series (the model's `MaxDatapoints` default, 100,800).
const MAX_POINTS: usize = 100_800;

/// The data of `GetMetricData`: the samples on disk, over the request's window.
struct Disk<'a> {
    core: &'a Core,
    window: (i64, i64),
}

impl Data for Disk<'_> {
    fn read(&self, s: &Stat) -> Result<Ts, String> {
        self.core.read_stat(s, self.window).map_err(|e| e.message)
    }

    fn search(&self, query: &Search, stat: &str, period_s: i64) -> Result<Vec<Ts>, String> {
        if !known(stat) {
            return Err(format!("The statistic {stat} is not supported."));
        }
        check_period(period_s).map_err(|e| e.message)?;
        let mut out = Vec::new();
        for (_, series) in self
            .core
            .series
            .iter()
            .filter(|(_, s)| math::matches(query, s))
        {
            let s = Stat {
                namespace: series.namespace.clone(),
                metric: series.name.clone(),
                dims: series.dims.clone(),
                stat: stat.to_owned(),
                unit: None,
                period_s,
            };
            let mut ts = self.read(&s)?;
            if ts.points.is_empty() {
                continue;
            }
            // Recorded 2026-11-19: a search result is labelled with its dimension values, not the metric name. A metric with no
            // dimensions falls back to its name (memory).
            ts.label = if series.dims.is_empty() {
                series.name.clone()
            } else {
                series
                    .dims
                    .iter()
                    .map(|(_, v)| v.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            out.push(ts);
            if out.len() >= math::MAX_SEARCH {
                break;
            }
        }
        Ok(out)
    }
}

impl Core {
    /// One metric folded into its period: a value per period, all units together unless the query names one.
    pub(crate) fn read_stat(&self, s: &Stat, window: (i64, i64)) -> Result<Ts, ApiError> {
        check_period(s.period_s)?;
        if !known(&s.stat) {
            // (memory)
            return Err(bad_value(format!(
                "The statistic {} is not supported.",
                s.stat
            )));
        }
        let key = series_key(&s.namespace, &s.metric, &s.dims);
        let unit = s
            .unit
            .as_deref()
            .and_then(|u| UNITS.iter().position(|x| *x == u));
        let folded = match self.series.get(&key) {
            Some(series) => self.fold(
                series.no,
                (align(window.0, s.period_s), window.1),
                s.period_s * 1000,
                unit,
                is_percentile(&s.stat),
            )?,
            None => BTreeMap::default(),
        };
        let mut per: BTreeMap<i64, Agg> = BTreeMap::new();
        for ((at, _), agg) in folded {
            per.entry(at).or_default().merge(&agg);
        }
        let points = per
            .iter()
            .filter_map(|(at, agg)| Some((*at, value_of(agg, &s.stat)?)))
            .collect();
        Ok(Ts::new(s.metric.clone(), points))
    }

    pub(crate) fn get_metric_data(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (start, end) = (
            r.time("StartTime").unwrap_or(0),
            r.time("EndTime").unwrap_or(0),
        );
        if start >= end {
            return Err(bad_value(
                "The parameter StartTime must be less than the parameter EndTime.",
            ));
        }
        let newest_first = r.opt("ScanBy") != Some("TimestampAscending");
        let raw =
            r.v.get("MetricDataQueries")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
        if raw.len() > MAX_QUERIES {
            // (memory)
            return Err(validation(format!(
                "The collection MetricDataQueries must not have a size greater than {MAX_QUERIES}."
            )));
        }
        let queries = math::read(raw).map_err(validation)?;
        let period_s = queries
            .iter()
            .filter_map(|q| match &q.body {
                Body::Stat(s) => Some(s.period_s),
                Body::Expr(_) => q.period_s,
            })
            .min()
            .unwrap_or(60);
        check_period(period_s)?;
        let grid = math::grid(align(start, period_s), end, period_s * 1000, MAX_POINTS);
        let disk = Disk {
            core: self,
            window: (start, end),
        };
        let values = math::solve(&queries, &grid, period_s * 1000, &disk).map_err(validation)?;
        let stamps = stat_stamps(&queries, &values, &grid);
        let mut results = Vec::new();
        for q in queries.iter().filter(|q| q.return_data) {
            let Some(v) = values.get(&q.id) else { continue };
            for ts in shown(q, v, &stamps)? {
                results.push(result_json(&q.id, &ts, newest_first));
            }
        }
        Ok(json!({"MetricDataResults": results, "Messages": []}))
    }
}

/// The timestamps the request's metrics have, where a scalar is shown; the window's periods when they have none.
fn stat_stamps(
    queries: &[Query],
    values: &std::collections::HashMap<String, Val>,
    grid: &[i64],
) -> Vec<i64> {
    let mut at = BTreeSet::new();
    for q in queries.iter().filter(|q| matches!(q.body, Body::Stat(_))) {
        if let Some(Val::Series(ts)) = values.get(&q.id) {
            at.extend(ts.points.keys().copied());
        }
    }
    if at.is_empty() {
        grid.to_vec()
    } else {
        at.into_iter().collect()
    }
}

/// The series a query's value is shown as, labelled.
fn shown(q: &Query, v: &Val, stamps: &[i64]) -> Result<Vec<Ts>, ApiError> {
    let own = |ts: &Ts, fallback: &str| {
        let mut ts = ts.clone();
        // (memory) an expression with no label is labelled with its id.
        ts.label = q.label.clone().unwrap_or_else(|| {
            if ts.label.is_empty() {
                fallback.to_owned()
            } else {
                ts.label.clone()
            }
        });
        ts
    };
    Ok(match v {
        Val::Series(ts) => match q.body {
            Body::Stat(_) => vec![ts.clone()],
            Body::Expr(_) => vec![own(
                &Ts {
                    label: String::new(),
                    ..ts.clone()
                },
                &q.id,
            )],
        },
        Val::Array(all) => all
            .iter()
            .map(|ts| {
                let mut ts = ts.clone();
                if ts.label.is_empty() {
                    ts.label.clone_from(&q.id);
                }
                ts
            })
            .collect(),
        Val::Scalar(x) => vec![own(&math::broadcast(*x, stamps), &q.id)],
        other => {
            return Err(validation(format!(
                "Error in expression '{}': the expression gives {}, not a time series",
                q.id,
                other.kind()
            )));
        }
    })
}

fn result_json(id: &str, ts: &Ts, newest_first: bool) -> Value {
    let mut points: Vec<(i64, f64)> = ts.points.iter().map(|(t, v)| (*t, *v)).collect();
    if newest_first {
        points.reverse();
    }
    json!({
        "Id": id,
        "Label": ts.label,
        "Timestamps": points.iter().map(|(t, _)| secs(*t)).collect::<Vec<_>>(),
        "Values": points.iter().map(|(_, v)| *v).collect::<Vec<_>>(),
        "StatusCode": "Complete",
    })
}
