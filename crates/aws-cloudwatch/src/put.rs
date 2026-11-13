//! `PutMetricData`, and the bounds on what is kept.

use std::collections::HashMap;

use aws_wire::ApiError;
use doze_store::record;
use serde_json::{Value, json};

use crate::core::Core;
use crate::model::{BUCKET_VERSION, Bucket, Series, UNITS, sample_key, series_key};
use crate::req::{Req, bad_value, dimensions};

/// Metric data points one call may carry.
const MAX_DATA: usize = 1000;
/// How far back a data point may be, and how far ahead (AWS: two weeks, two hours).
const PAST_MS: i64 = 14 * 86_400_000;
pub(crate) const FUTURE_MS: i64 = 2 * 3_600_000;
/// Samples are kept for `--cloudwatch-retention-hours` (default a day: AWS keeps 15 days of one-minute data, and a local store is
/// disposable; see [`crate::Settings`]), and one-second data at most this long (AWS: three hours).
const KEEP_HIGH_RES_MS: i64 = 3 * 3_600_000;
/// A metric with no new data point for this long is forgotten (AWS: two weeks).
const KEEP_SERIES_MS: i64 = 14 * 86_400_000;
/// Metrics kept at once; past it the one updated longest ago goes.
const MAX_SERIES: usize = 10_000;
/// Old samples are looked for at most this often.
const TRIM_EVERY_MS: i64 = 600_000;
/// Width of a bucket of ordinary and of high-resolution data.
pub(crate) const BUCKET_MS: i64 = 60_000;
pub(crate) const BUCKET_HIGH_MS: i64 = 1_000;

/// The words AWS used for a bad unit (recorded 2026-11-09), whatever the member's index.
pub(crate) const UNIT_MESSAGE: &str = "The parameter MetricData.member.1.Unit must be a value in the set [ Megabits, Terabits, Gigabits, Count, Bytes, Gigabytes, Gigabytes/Second, Kilobytes, Kilobits/Second, Terabytes, Terabits/Second, Bytes/Second, Percent, Megabytes, Megabits/Second, Milliseconds, Microseconds, Kilobytes/Second, Gigabits/Second, Megabytes/Second, Bits, Bits/Second, Count/Second, Seconds, Kilobits, Terabytes/Second, None ].";

/// One point of a request, read and checked.
struct Point {
    name: String,
    dims: Vec<(String, String)>,
    at_ms: i64,
    unit: usize,
    high_res: bool,
    /// What the point contributes: the four statistics of a set, or observations of one value.
    add: Contribution,
}

enum Contribution {
    Set(Bucket),
    Values(Vec<(f64, f64)>),
}

impl Core {
    pub(crate) fn put_metric_data(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let namespace = r.s("Namespace").to_owned();
        let data =
            r.v.get("MetricData")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
        if data.len() > MAX_DATA {
            // (memory)
            return Err(ApiError::modeled(
                "InvalidParameterCombinationException",
                format!("The collection MetricData must not have a size greater than {MAX_DATA}."),
            ));
        }
        let now = self.now_ms();
        let points = data
            .iter()
            .enumerate()
            .map(|(i, d)| point(d, i + 1, now))
            .collect::<Result<Vec<_>, _>>()?;
        let mut touched = Vec::new();
        let mut buckets: HashMap<String, Bucket> = HashMap::new();
        for p in points {
            let key = series_key(&namespace, &p.name, &p.dims);
            let no = self.series_for(&key, &namespace, &p, now);
            let width = if p.high_res {
                BUCKET_HIGH_MS
            } else {
                BUCKET_MS
            };
            let row = sample_key(no, p.high_res, p.at_ms - p.at_ms.rem_euclid(width), p.unit);
            if !buckets.contains_key(&row) {
                let existing = self.bucket(&row)?;
                buckets.insert(row.clone(), existing);
            }
            if let Some(b) = buckets.get_mut(&row) {
                match &p.add {
                    Contribution::Set(set) => b.merge(set),
                    Contribution::Values(vs) => {
                        for (v, c) in vs {
                            b.add(*v, *c);
                        }
                    }
                }
            }
            if !touched.contains(&key) {
                touched.push(key);
            }
        }
        for (row, b) in buckets {
            let bytes = record::encode("bucket", BUCKET_VERSION, &b)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            self.samples.put(row, bytes);
        }
        self.trim(now);
        self.note_written(touched);
        Ok(json!({}))
    }

    /// The row of a bucket as it is now (an empty one if there is none).
    fn bucket(&self, row: &str) -> Result<Bucket, ApiError> {
        match self
            .samples
            .get(self.db.as_ref(), row)
            .map_err(|e| ApiError::internal(e.to_string()))?
        {
            Some(bytes) => Bucket::decode(&bytes).map_err(|e| ApiError::internal(e.to_string())),
            None => Ok(Bucket::default()),
        }
    }

    /// The number of the series (made if it is new) and its newest sample time moved on.
    fn series_for(&mut self, key: &str, namespace: &str, p: &Point, now: i64) -> u32 {
        if let Some(s) = self.series.get_mut(key) {
            s.last_ms = s.last_ms.max(p.at_ms.min(now));
            return s.no;
        }
        let no = self
            .series
            .iter()
            .map(|(_, s)| s.no)
            .max()
            .map_or(1, |n| n + 1);
        self.series.insert(
            key.to_owned(),
            Series {
                no,
                namespace: namespace.to_owned(),
                name: p.name.clone(),
                dims: p.dims.clone(),
                last_ms: p.at_ms.min(now),
            },
        );
        no
    }

    /// Removes samples past what is kept, the metrics that have gone quiet, and the oldest metrics past the cap. At most every ten
    /// minutes, and only when something is written.
    fn trim(&mut self, now: i64) {
        if now - self.trimmed_ms < TRIM_EVERY_MS {
            return;
        }
        self.trimmed_ms = now;
        let quiet: Vec<String> = self
            .series
            .iter()
            .filter(|(_, s)| now - s.last_ms > KEEP_SERIES_MS)
            .map(|(k, _)| k.to_owned())
            .collect();
        let mut victims = quiet;
        let over = self.series.len().saturating_sub(MAX_SERIES + victims.len());
        if over > 0 {
            let mut by_age: Vec<(i64, String)> = self
                .series
                .iter()
                .filter(|(k, _)| !victims.iter().any(|v| v == k))
                .map(|(k, s)| (s.last_ms, k.to_owned()))
                .collect();
            by_age.sort();
            victims.extend(by_age.into_iter().take(over).map(|(_, k)| k));
        }
        for key in victims {
            if let Some(s) = self.series.remove(&key) {
                self.samples
                    .delete_range(&format!("{:08x}", s.no), &format!("{:08x}", s.no + 1));
            }
        }
        for (_, s) in self.series.iter() {
            self.samples.delete_range(
                &sample_key(s.no, false, 0, 0),
                &sample_key(s.no, false, now - self.keep_ms, 0),
            );
            self.samples.delete_range(
                &sample_key(s.no, true, 0, 0),
                &sample_key(s.no, true, now - KEEP_HIGH_RES_MS.min(self.keep_ms), 0),
            );
        }
    }
}

/// A data point of a request, checked in AWS's words. `n` counts from 1, as AWS's messages do.
fn point(d: &Value, n: usize, now: i64) -> Result<Point, ApiError> {
    let name = d
        .get("MetricName")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let has = |k: &str| d.get(k).is_some_and(|v| !v.is_null());
    // Recorded on AWS (2026-11-09).
    if has("Value") && has("StatisticValues") {
        return Err(ApiError::modeled(
            "InvalidParameterCombinationException",
            format!(
                "The parameters MetricData.member.{n}.Value and MetricData.member.{n}.StatisticValues are mutually exclusive and you have specified both."
            ),
        ));
    }
    let at_ms = d
        .get("Timestamp")
        .and_then(Value::as_f64)
        .map_or(now, crate::req::secs_to_ms);
    if at_ms < now - PAST_MS || at_ms > now + FUTURE_MS {
        // (memory)
        return Err(bad_value(format!(
            "The parameter MetricData.member.{n}.Timestamp must specify a time within the past two weeks and no more than two hours into the future."
        )));
    }
    let unit = d
        .get("Unit")
        .and_then(Value::as_str)
        .and_then(|u| UNITS.iter().position(|x| *x == u))
        .unwrap_or(0);
    Ok(Point {
        name: name.to_owned(),
        dims: dimensions(d.get("Dimensions")),
        at_ms,
        unit,
        high_res: d.get("StorageResolution").and_then(Value::as_i64) == Some(1),
        add: contribution(d, n)?,
    })
}

fn contribution(d: &Value, n: usize) -> Result<Contribution, ApiError> {
    let number = |v: &Value, field: &str| -> Result<f64, ApiError> {
        let f = v.as_f64().unwrap_or(f64::NAN);
        if f.is_finite() {
            Ok(f)
        } else {
            // (memory)
            Err(bad_value(format!(
                "The value {f} for parameter MetricData.member.{n}.{field} is invalid."
            )))
        }
    };
    if let Some(set) = d.get("StatisticValues").filter(|v| v.is_object()) {
        let get = |k: &str| number(set.get(k).unwrap_or(&Value::Null), "StatisticValues");
        return Ok(Contribution::Set(Bucket {
            count: get("SampleCount")?,
            sum: get("Sum")?,
            min: get("Minimum")?,
            max: get("Maximum")?,
            values: Vec::new(),
            seen: 0.0,
        }));
    }
    if let Some(v) = d.get("Value").filter(|v| !v.is_null()) {
        return Ok(Contribution::Values(vec![(number(v, "Value")?, 1.0)]));
    }
    let values = d
        .get("Values")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if values.is_empty() {
        // (memory)
        return Err(ApiError::modeled(
            "InvalidParameterCombinationException",
            format!(
                "At least one of the parameters MetricData.member.{n}.Value, MetricData.member.{n}.Values or MetricData.member.{n}.StatisticValues must be specified."
            ),
        ));
    }
    let counts = d
        .get("Counts")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if !counts.is_empty() && counts.len() != values.len() {
        // (memory)
        return Err(ApiError::modeled(
            "InvalidParameterCombinationException",
            format!(
                "The parameters MetricData.member.{n}.Values and MetricData.member.{n}.Counts must have the same number of elements."
            ),
        ));
    }
    let mut out = Vec::new();
    for (i, v) in values.iter().enumerate() {
        let c = counts.get(i).and_then(Value::as_f64).unwrap_or(1.0);
        out.push((number(v, "Values")?, c));
    }
    Ok(Contribution::Values(out))
}
