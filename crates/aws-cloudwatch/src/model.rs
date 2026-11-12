//! What CloudWatch holds: metric series and their samples, alarms and their history, and stored documents.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A metric: namespace, name and dimensions. A different dimension set is a different metric.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Series {
    /// The number that prefixes the series' sample rows.
    pub(crate) no: u32,
    pub(crate) namespace: String,
    pub(crate) name: String,
    pub(crate) dims: Vec<(String, String)>,
    /// The time of the newest sample.
    pub(crate) last_ms: i64,
}

/// The key of a series: namespace, name and the sorted dimensions, with bytes none of them may hold between.
pub(crate) fn series_key(namespace: &str, name: &str, dims: &[(String, String)]) -> String {
    let mut key = format!("{namespace}\u{1}{name}");
    for (n, v) in dims {
        key.push('\u{1}');
        key.push_str(n);
        key.push('\u{2}');
        key.push_str(v);
    }
    key
}

/// The samples of one series that fell in one bucket of time, in one unit. The statistics are exact; `values` keeps up to
/// [`MAX_VALUES`] of the raw values, for percentiles.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Bucket {
    pub(crate) count: f64,
    pub(crate) sum: f64,
    pub(crate) min: f64,
    pub(crate) max: f64,
    pub(crate) values: Vec<f64>,
}

/// Raw values kept per bucket.
pub(crate) const MAX_VALUES: usize = 128;

impl Bucket {
    /// Adds a sample of `count` observations of `value` (a statistic set arrives as its four numbers, see [`Bucket::merge`]).
    pub(crate) fn add(&mut self, value: f64, count: f64) {
        self.merge(&Self {
            count,
            sum: value * count,
            min: value,
            max: value,
            values: Vec::new(),
        });
        // The raw values keep one entry per observation (up to the cap), so a percentile can be read.
        let mut n = 0.0;
        while n < count && self.values.len() < MAX_VALUES {
            self.values.push(value);
            n += 1.0;
        }
    }

    pub(crate) fn merge(&mut self, other: &Self) {
        if self.count == 0.0 {
            self.min = other.min;
            self.max = other.max;
        } else {
            self.min = self.min.min(other.min);
            self.max = self.max.max(other.max);
        }
        self.count += other.count;
        self.sum += other.sum;
    }
}

/// The units, in the order a sample row names them (the row key holds the index).
pub(crate) const UNITS: [&str; 27] = [
    "None",
    "Seconds",
    "Microseconds",
    "Milliseconds",
    "Bytes",
    "Kilobytes",
    "Megabytes",
    "Gigabytes",
    "Terabytes",
    "Bits",
    "Kilobits",
    "Megabits",
    "Gigabits",
    "Terabits",
    "Percent",
    "Count",
    "Bytes/Second",
    "Kilobytes/Second",
    "Megabytes/Second",
    "Gigabytes/Second",
    "Terabytes/Second",
    "Bits/Second",
    "Kilobits/Second",
    "Megabits/Second",
    "Gigabits/Second",
    "Terabits/Second",
    "Count/Second",
];

/// The row key of a bucket: series, resolution, the bucket's start, and the unit, as fixed-width hex.
pub(crate) fn sample_key(series: u32, high_res: bool, bucket_ms: i64, unit: usize) -> String {
    format!(
        "{series:08x}{}{:016x}{unit:02x}",
        u8::from(high_res),
        bucket_ms.max(0)
    )
}

/// What an evaluation reads of an alarm: the metric, and how to judge it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Spec {
    pub(crate) namespace: String,
    pub(crate) metric: String,
    pub(crate) dims: Vec<(String, String)>,
    /// `Average`, `Sum`, `Minimum`, `Maximum`, `SampleCount`, or an extended statistic such as `p99`.
    pub(crate) stat: String,
    pub(crate) unit: Option<String>,
    pub(crate) period_s: i64,
    pub(crate) evaluation_periods: i64,
    pub(crate) datapoints_to_alarm: i64,
    pub(crate) threshold: f64,
    pub(crate) comparison: String,
    pub(crate) treat_missing: String,
}

/// What kind of alarm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Kind {
    /// One metric, judged by a statistic.
    Metric,
    /// Metrics combined by an expression: stored, not evaluated.
    Math,
    /// Alarms combined by a rule: stored, not evaluated.
    Composite,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Alarm {
    pub(crate) kind: Kind,
    /// The request as it was sent (without the tags): the alarm is shown from it, so nothing sent is lost.
    pub(crate) request: String,
    pub(crate) spec: Spec,
    pub(crate) tags: BTreeMap<String, String>,
    pub(crate) state: String,
    pub(crate) reason: String,
    pub(crate) reason_data: Option<String>,
    pub(crate) config_updated_ms: i64,
    pub(crate) state_updated_ms: i64,
    pub(crate) transitioned_ms: i64,
    pub(crate) actions_enabled: bool,
    pub(crate) ok_actions: Vec<String>,
    pub(crate) alarm_actions: Vec<String>,
    pub(crate) insufficient_actions: Vec<String>,
}

/// One entry of an alarm's history.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Item {
    pub(crate) at_ms: i64,
    /// `ConfigurationUpdate`, `StateUpdate` or `Action`.
    pub(crate) kind: String,
    pub(crate) summary: String,
    pub(crate) data: String,
    /// `MetricAlarm` or `CompositeAlarm`.
    pub(crate) alarm_type: String,
}

/// An alarm's history, newest last, bounded.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct History {
    pub(crate) items: Vec<Item>,
}

/// History kept per alarm.
pub(crate) const MAX_HISTORY: usize = 100;

/// A document kept as sent: a dashboard, an anomaly detector. Stored, returned, and applied to nothing.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Doc {
    pub(crate) json: String,
    pub(crate) updated_ms: i64,
    pub(crate) tags: BTreeMap<String, String>,
}

pub(crate) fn doc_key(kind: &str, id: &str) -> String {
    format!("{kind}\u{1}{id}")
}
