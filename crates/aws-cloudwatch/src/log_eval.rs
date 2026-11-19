//! Log alarms, evaluated. As the model documents `PutLogAlarm`: the alarm runs a CloudWatch Logs scheduled query on its `rate()`
//! schedule over `[run - StartTimeOffset, run - EndTimeOffset]`; "each scheduled query execution returns one or more aggregated values
//! determined by the `AggregationExpression`, and each aggregated value is compared against the alarm `Threshold`"; and the alarm uses
//! "M-out-of-N evaluation: if `QueryResultsToAlarm` out of the most recent `QueryResultsToEvaluate` query results breach the
//! threshold, the alarm transitions to `ALARM`".
//!
//! Here the actor only keeps time: at each run it hands the timer the query to make (`LogRun`), which is run through the peer bus as
//! Logs Insights' `StartQuery` (the query string, then `| stats <AggregationExpression>`), outside the actor, so CloudWatch never
//! waits on Logs while Logs may be waiting on it (metric filters). The answer comes back as a `LogOutcome` and is judged as a metric
//! alarm's window is (`judge`), one result per run. A run whose query has several contributors (`... by host`) is breaching when any
//! of them is (the highest value for a greater-than comparison, the lowest for a less-than one): contributors are not kept apart
//! (difference). A run that finds no rows is missing data, judged by `TreatMissingData`. A query that fails makes the alarm
//! `INSUFFICIENT_DATA` with `EvaluationState: EVALUATION_ERROR` and the failure as its reason (memory).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::Core;
use crate::judge::{Window, judge};
use crate::model::{Alarm, Doc, Kind, Spec, doc_key};

/// Where a log alarm's runs are kept, by alarm name.
const KEPT: &str = "logalarm";
/// Results kept per alarm: the most `QueryResultsToEvaluate` may be (the model: 1 through 100).
const MAX_RUNS: usize = 100;

/// A scheduled query a log alarm needs run.
#[derive(Debug, Clone)]
pub(crate) struct LogRun {
    pub(crate) alarm: String,
    /// The alarm's configuration time when the run was made: an answer for an alarm since changed is dropped.
    pub(crate) config_ms: i64,
    /// The scheduled time of the run.
    pub(crate) at_ms: i64,
    pub(crate) query: String,
    pub(crate) groups: Vec<String>,
    /// The query's time range, in milliseconds.
    pub(crate) range_ms: (i64, i64),
    /// The column the aggregate is shown under, and the `by` fields.
    pub(crate) value_field: String,
    pub(crate) by: Vec<String>,
}

/// What a run found: the aggregated values (one per contributor), or why the query failed.
#[derive(Debug)]
pub(crate) struct LogOutcome {
    pub(crate) run: LogRun,
    pub(crate) result: Result<Vec<f64>, String>,
}

/// What is kept of a log alarm between runs.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Kept {
    /// The configuration these runs belong to; a put starts afresh.
    config_ms: i64,
    last_ms: Option<i64>,
    runs: Vec<Run>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Run {
    at_ms: i64,
    value: Option<f64>,
}

/// A log alarm's configuration, read from its request.
struct Config {
    rate_ms: i64,
    offsets_ms: (i64, i64),
    query: String,
    groups: Vec<String>,
    aggregation: String,
    spec: Spec,
}

/// `rate(5 minutes)` in milliseconds (the forms `log_alarm::check` accepts).
fn rate_ms(expr: &str) -> Option<i64> {
    let (n, unit) = expr
        .strip_prefix("rate(")?
        .strip_suffix(')')?
        .trim()
        .split_once(' ')?;
    let n: i64 = n.parse().ok().filter(|n| *n > 0)?;
    let unit_ms = match unit.trim() {
        "minute" | "minutes" => 60_000,
        "hour" | "hours" => 3_600_000,
        "day" | "days" => 86_400_000,
        _ => return None,
    };
    n.checked_mul(unit_ms)
}

fn config(a: &Alarm) -> Option<Config> {
    let v: Value = serde_json::from_str(&a.request).ok()?;
    let q = v.get("ScheduledQueryConfiguration")?;
    let s = q.get("ScheduleConfiguration")?;
    let text = |o: &Value, k: &str| {
        o.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let secs = |k: &str| {
        s.get(k)
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .saturating_mul(1000)
    };
    let rate = rate_ms(s.get("ScheduleExpression")?.as_str()?)?;
    let n = v
        .get("QueryResultsToEvaluate")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    Some(Config {
        rate_ms: rate,
        offsets_ms: (secs("StartTimeOffset"), secs("EndTimeOffset")),
        query: text(q, "QueryString"),
        groups: q
            .get("LogGroupIdentifiers")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        aggregation: text(q, "AggregationExpression"),
        spec: Spec {
            period_s: rate / 1000,
            evaluation_periods: n,
            datapoints_to_alarm: v
                .get("QueryResultsToAlarm")
                .and_then(Value::as_i64)
                .unwrap_or(n),
            threshold: v.get("Threshold").and_then(Value::as_f64).unwrap_or(0.0),
            comparison: text(&v, "ComparisonOperator"),
            treat_missing: v
                .get("TreatMissingData")
                .and_then(Value::as_str)
                .unwrap_or("missing")
                .to_owned(),
            ..Spec::default()
        },
    })
}

/// The query a run makes, the column its aggregate is shown under (an `as` name, or the expression as written: Insights' naming),
/// and the `by` fields. A `sort` in the aggregation only orders contributors, which a verdict does not need, so it is left out.
fn compose(query: &str, aggregation: &str) -> (String, String, Vec<String>) {
    let mut parts = aggregation.split('|');
    let stats = parts.next().unwrap_or_default().trim().to_owned();
    let rest: String = parts
        .map(str::trim)
        .filter(|p| !p.is_empty() && !p.to_ascii_lowercase().starts_with("sort"))
        .map(|p| format!(" | {p}"))
        .collect();
    let lower = stats.to_ascii_lowercase();
    let (expr, by) = match lower.find(" by ") {
        Some(at) => (
            stats.get(..at).unwrap_or_default(),
            stats.get(at + 4..).unwrap_or_default(),
        ),
        None => (stats.as_str(), ""),
    };
    let lower_expr = expr.to_ascii_lowercase();
    let field = match lower_expr.rfind(" as ") {
        Some(at) => expr.get(at + 4..).unwrap_or_default(),
        None => expr,
    };
    let by = by
        .split(',')
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .map(str::to_owned)
        .collect();
    (
        format!("{query} | stats {stats}{rest}"),
        field.trim().to_owned(),
        by,
    )
}

impl Core {
    fn kept_runs(&self, name: &str) -> Kept {
        self.docs
            .get(&doc_key(KEPT, name))
            .and_then(|d| serde_json::from_str(&d.json).ok())
            .unwrap_or_default()
    }

    fn keep_runs(&mut self, name: &str, kept: &Kept) {
        let updated_ms = self.now_ms();
        if let Ok(json) = serde_json::to_string(kept) {
            self.docs.insert(
                doc_key(KEPT, name),
                Doc {
                    json,
                    updated_ms,
                    tags: Default::default(),
                },
            );
        }
    }

    /// Forgets a deleted log alarm's runs.
    pub(crate) fn forget_log_runs(&mut self, name: &str) {
        self.docs.remove(&doc_key(KEPT, name));
    }

    /// What `DescribeAlarms` adds to a log alarm: `EvaluationState` while its last query failed.
    pub(crate) fn evaluation_state(&self, name: &str) -> Option<&'static str> {
        self.kept_runs(name).error.map(|_| "EVALUATION_ERROR")
    }

    /// When the log alarm called `name` next runs: a rate after its last run, or after it was put.
    pub(crate) fn log_due(&self, name: &str) -> Option<i64> {
        let a = self.alarms.get(name).filter(|a| a.kind == Kind::Log)?;
        let c = config(a)?;
        let kept = self.kept_runs(name);
        let last = kept
            .last_ms
            .filter(|_| kept.config_ms == a.config_updated_ms)
            .unwrap_or(a.config_updated_ms);
        Some(last.saturating_add(c.rate_ms))
    }

    /// Makes the run that is due: on the schedule's grid (a rate apart from when the alarm was put), the latest point not after now,
    /// so a run missed while doze-aws was stopped is made once and not caught up on.
    pub(crate) fn start_log_run(&mut self, name: &str, now: i64) {
        let Some((a, c)) = self
            .alarms
            .get(name)
            .and_then(|a| Some((a.config_updated_ms, config(a)?)))
        else {
            return;
        };
        let at = a + (now - a).div_euclid(c.rate_ms).max(1) * c.rate_ms;
        let mut kept = self.kept_runs(name);
        if kept.config_ms != a {
            kept = Kept {
                config_ms: a,
                ..Kept::default()
            };
        }
        kept.last_ms = Some(at);
        self.keep_runs(name, &kept);
        let (query, value_field, by) = compose(&c.query, &c.aggregation);
        self.log_runs.push(LogRun {
            alarm: name.to_owned(),
            config_ms: a,
            at_ms: at,
            query,
            groups: c.groups,
            range_ms: (at - c.offsets_ms.0, at - c.offsets_ms.1),
            value_field,
            by,
        });
        self.refresh_due(name, false);
    }

    /// Takes a run's answer in and judges the alarm on its newest results.
    pub(crate) fn apply_log_outcome(&mut self, o: LogOutcome) {
        let name = o.run.alarm.as_str();
        let Some((current, c)) = self
            .alarms
            .get(name)
            .filter(|a| a.kind == Kind::Log && a.config_updated_ms == o.run.config_ms)
            .and_then(|a| Some((a.state.clone(), config(a)?)))
        else {
            return;
        };
        let now = self.now_ms();
        let mut kept = self.kept_runs(name);
        let values = match o.result {
            Ok(v) => v,
            Err(message) => {
                // (memory) the reason of an alarm whose query could not run.
                let reason = format!("The scheduled query could not be evaluated: {message}");
                kept.error = Some(message);
                self.keep_runs(name, &kept);
                self.move_to(
                    name,
                    "INSUFFICIENT_DATA",
                    (&reason, None),
                    now,
                    current != "INSUFFICIENT_DATA",
                );
                return;
            }
        };
        let greater = c.spec.comparison.starts_with("Greater");
        let value = values
            .into_iter()
            .reduce(|x, y| if (y > x) == greater { y } else { x });
        kept.error = None;
        kept.runs.retain(|r| r.at_ms != o.run.at_ms);
        kept.runs.push(Run {
            at_ms: o.run.at_ms,
            value,
        });
        kept.runs.sort_by_key(|r| r.at_ms);
        let over = kept.runs.len().saturating_sub(MAX_RUNS);
        kept.runs.drain(..over);
        let window: Window = kept
            .runs
            .iter()
            .filter_map(|r| Some((r.at_ms, r.value?)))
            .collect();
        let newest = kept.runs.last().map_or(o.run.at_ms, |r| r.at_ms);
        self.keep_runs(name, &kept);
        let v = judge(&c.spec, &window, None, newest + c.rate_ms, now);
        if let Some(state) = v.state.filter(|s| *s != current) {
            self.move_to(name, state, (&v.reason, Some(&v.data)), now, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_aggregation_becomes_a_stats_command_and_names_its_column() {
        assert_eq!(
            compose("fields @message", "count(*)"),
            (
                "fields @message | stats count(*)".to_owned(),
                "count(*)".to_owned(),
                vec![]
            )
        );
        assert_eq!(
            compose("filter x", "avg(latency) as lat by host, az | sort desc"),
            (
                "filter x | stats avg(latency) as lat by host, az".to_owned(),
                "lat".to_owned(),
                vec!["host".to_owned(), "az".to_owned()]
            )
        );
        assert_eq!(rate_ms("rate(5 minutes)"), Some(300_000));
        assert_eq!(rate_ms("rate(1 hour)"), Some(3_600_000));
        assert_eq!(rate_ms("cron(0 * * * ? *)"), None);
    }
}
