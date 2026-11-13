//! Reading metrics: `GetMetricStatistics`, `GetMetricData` and `ListMetrics`.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::model::{Series, UNITS, series_key};
use crate::req::{Put, Req, bad_value, dimension_list, dimensions, secs};
use crate::stats::{Agg, is_percentile, known, value_of};

/// Datapoints one `GetMetricStatistics` may return (AWS: 1,440).
const MAX_POINTS: i64 = 1440;
/// Metrics one `ListMetrics` page holds (AWS: 500).
const LIST_PAGE: usize = 500;
/// The periods under a minute that high-resolution data answers.
const SHORT_PERIODS: [i64; 5] = [1, 5, 10, 20, 30];

impl Core {
    pub(crate) fn get_metric_statistics(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (start, end) = (
            r.time("StartTime").unwrap_or(0),
            r.time("EndTime").unwrap_or(0),
        );
        let period = r.int("Period").unwrap_or(60);
        let stats = r.strings("Statistics");
        let extended = r.strings("ExtendedStatistics");
        if stats.is_empty() && extended.is_empty() {
            // Recorded on AWS (2026-11-09).
            return Err(ApiError::modeled(
                "InvalidParameterCombinationException",
                "At least one of the parameters Statistics and ExtendedStatistics must be specified.",
            ));
        }
        if start >= end {
            // Recorded on AWS (2026-11-09).
            return Err(bad_value(
                "The parameter StartTime must be less than the parameter EndTime.",
            ));
        }
        check_period(period)?;
        let points = (end - start) / (period * 1000);
        if points > MAX_POINTS {
            // (memory)
            return Err(ApiError::modeled(
                "InvalidParameterCombinationException",
                format!(
                    "You have requested up to {points} datapoints, which exceeds the limit of {MAX_POINTS}. You may reduce the datapoints requested by increasing Period, or decreasing the time range."
                ),
            ));
        }
        if let Some(bad) = extended.iter().find(|e| !known(e)) {
            // (memory)
            return Err(bad_value(format!(
                "The extended statistic {bad} is not supported."
            )));
        }
        let name = r.s("MetricName");
        let key = series_key(r.s("Namespace"), name, &r.dimensions());
        let unit = r
            .opt("Unit")
            .and_then(|u| UNITS.iter().position(|x| *x == u));
        let folded = match self.series.get(&key) {
            Some(s) => self.fold(
                s.no,
                (align(start, period), end),
                period * 1000,
                unit,
                !extended.is_empty(),
            )?,
            None => Default::default(),
        };
        let datapoints: Vec<Value> = folded
            .iter()
            .map(|((at, u), agg)| {
                let mut d = json!({"Timestamp": secs(*at), "Unit": UNITS.get(*u).copied().unwrap_or("None")});
                for s in &stats {
                    if let Some(v) = value_of(agg, s) {
                        d.put(s, json!(v));
                    }
                }
                let ext: serde_json::Map<String, Value> = extended
                    .iter()
                    .filter_map(|e| Some((e.clone(), json!(value_of(agg, e)?))))
                    .collect();
                if !ext.is_empty() {
                    d.put("ExtendedStatistics", Value::Object(ext));
                }
                d
            })
            .collect();
        Ok(json!({"Label": name, "Datapoints": datapoints}))
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
        let queries =
            r.v.get("MetricDataQueries")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
        let mut results = Vec::new();
        for q in queries {
            let Some(stat) = q.get("MetricStat") else {
                // Metric math is not evaluated here: refused by name, not answered with a guess.
                return Err(ApiError::new(
                    501,
                    "NotImplemented",
                    "Metric math expressions are not evaluated in doze-aws.",
                ));
            };
            if q.get("ReturnData").and_then(Value::as_bool) == Some(false) {
                continue;
            }
            results.push(self.query_result(q, stat, (start, end), newest_first)?);
        }
        Ok(json!({"MetricDataResults": results, "Messages": []}))
    }

    /// One query of `GetMetricData`: the series, folded into its period, as timestamps and values.
    fn query_result(
        &self,
        q: &Value,
        stat: &Value,
        window: (i64, i64),
        newest_first: bool,
    ) -> Result<Value, ApiError> {
        let metric = stat.get("Metric").unwrap_or(&Value::Null);
        let text = |v: &Value, k: &str| {
            v.get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let name = text(metric, "MetricName");
        let period = stat.get("Period").and_then(Value::as_i64).unwrap_or(60);
        check_period(period)?;
        let which = text(stat, "Stat");
        if !known(&which) {
            // (memory)
            return Err(bad_value(format!(
                "The statistic {which} is not supported."
            )));
        }
        let key = series_key(
            &text(metric, "Namespace"),
            &name,
            &dimensions(metric.get("Dimensions")),
        );
        let unit = stat
            .get("Unit")
            .and_then(Value::as_str)
            .and_then(|u| UNITS.iter().position(|x| *x == u));
        let folded = match self.series.get(&key) {
            Some(s) => self.fold(
                s.no,
                (align(window.0, period), window.1),
                period * 1000,
                unit,
                is_percentile(&which),
            )?,
            None => Default::default(),
        };
        // One value per period: the units of a period are folded together here, as a query names no unit.
        let mut per: std::collections::BTreeMap<i64, Agg> = std::collections::BTreeMap::new();
        for ((at, _), agg) in folded {
            per.entry(at).or_default().merge(&agg);
        }
        let mut points: Vec<(i64, f64)> = per
            .iter()
            .filter_map(|(at, agg)| Some((*at, value_of(agg, &which)?)))
            .collect();
        if newest_first {
            points.reverse();
        }
        Ok(json!({
            "Id": text(q, "Id"),
            "Label": q.get("Label").and_then(Value::as_str).unwrap_or(&name),
            "Timestamps": points.iter().map(|(t, _)| secs(*t)).collect::<Vec<_>>(),
            "Values": points.iter().map(|(_, v)| *v).collect::<Vec<_>>(),
            "StatusCode": "Complete",
        }))
    }

    pub(crate) fn list_metrics(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (namespace, name) = (r.opt("Namespace"), r.opt("MetricName"));
        let wanted: Vec<(String, Option<String>)> =
            r.v.get("Dimensions")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .map(|d| {
                            (
                                d.get("Name")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_owned(),
                                d.get("Value").and_then(Value::as_str).map(str::to_owned),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
        let recent = r
            .opt("RecentlyActive")
            .map(|_| self.now_ms() - 3 * 3_600_000);
        let after = r.opt("NextToken");
        let matches = |s: &Series| {
            namespace.is_none_or(|n| s.namespace == n)
                && name.is_none_or(|n| s.name == n)
                && recent.is_none_or(|t| s.last_ms >= t)
                && wanted.iter().all(|(n, v)| {
                    s.dims
                        .iter()
                        .any(|(sn, sv)| sn == n && v.as_ref().is_none_or(|v| v == sv))
                })
        };
        let rest: Vec<(&str, &Series)> = self
            .series
            .iter()
            .filter(|(k, s)| matches(s) && after.is_none_or(|a| *k > a))
            .collect();
        let page: Vec<Value> = rest
            .iter()
            .take(LIST_PAGE)
            .map(|(_, s)| {
                json!({"Namespace": s.namespace, "MetricName": s.name, "Dimensions": dimension_list(&s.dims)})
            })
            .collect();
        let mut out = json!({ "Metrics": page });
        if let (true, Some((key, _))) = (rest.len() > LIST_PAGE, rest.get(LIST_PAGE - 1)) {
            out.put("NextToken", json!(key));
        }
        Ok(out)
    }
}

/// The period a request may name: a whole number of minutes, or one of the short periods high-resolution data answers.
fn check_period(period: i64) -> Result<(), ApiError> {
    if period < 60 && !SHORT_PERIODS.contains(&period) {
        // Recorded on AWS (2026-11-09) for a period of 45.
        return Err(bad_value(
            "The parameter Period must be a value in the set [1, 5, 10, 20, 30].",
        ));
    }
    if period >= 60 && period % 60 != 0 {
        // (memory)
        return Err(bad_value("The parameter Period must be a multiple of 60."));
    }
    Ok(())
}

/// A start time rounded down to the whole minute (the second, for a short period), as AWS rounds it.
fn align(start_ms: i64, period_s: i64) -> i64 {
    let unit = if period_s < 60 { 1000 } else { 60_000 };
    start_ms - start_ms.rem_euclid(unit)
}
