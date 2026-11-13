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

/// The samples of one series that fell in one bucket of time, in one unit. The statistics are exact. `values` is for percentiles:
/// every observation given as a value while there are no more than [`MAX_VALUES`] of them (the percentile is then exact), and past
/// that a uniform random sample of [`MAX_VALUES`] of the `seen` observations (reservoir sampling), each standing for `seen / len`.
/// The error of a percentile read from the sample is a rank error of about `sqrt(p (1 - p) / 1024)`: for p99 about 0.3 percentile
/// points, for p50 about 1.6. A statistic set (`StatisticValues`) has no values and counts in no percentile, as on AWS.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Bucket {
    pub(crate) count: f64,
    pub(crate) sum: f64,
    pub(crate) min: f64,
    pub(crate) max: f64,
    pub(crate) values: Vec<f64>,
    /// Observations given as values, of which `values` holds all or a sample.
    pub(crate) seen: f64,
}

/// The first shape of a bucket (record version 1): the raw values were the first 128, with no count of the ones left out.
#[derive(Deserialize)]
struct BucketV1 {
    count: f64,
    sum: f64,
    min: f64,
    max: f64,
    values: Vec<f64>,
}

/// The record version of a bucket (1 had no `seen`).
pub(crate) const BUCKET_VERSION: u8 = 2;

/// Raw values kept per bucket: 8 KiB of a row at most.
pub(crate) const MAX_VALUES: usize = 1024;
/// Observations of one value past the sample's size that are sampled one by one; more are sampled by their expected number of
/// replacements.
const EXACT_DRAWS: f64 = 4096.0;

impl Bucket {
    /// Reads a stored bucket of either version.
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, doze_store::StoreError> {
        let (version, body) = doze_store::record::split("bucket", bytes)?;
        if version == 1 {
            let old: BucketV1 = doze_store::record::body("bucket", body)?;
            #[allow(clippy::cast_precision_loss)]
            let seen = old.values.len() as f64;
            return Ok(Self {
                count: old.count,
                sum: old.sum,
                min: old.min,
                max: old.max,
                values: old.values,
                seen,
            });
        }
        doze_store::record::decode("bucket", BUCKET_VERSION, bytes)
    }

    /// Adds a sample of `count` observations of `value` (a statistic set arrives as its four numbers, see [`Bucket::merge`]).
    pub(crate) fn add(&mut self, value: f64, count: f64) {
        self.merge(&Self {
            count,
            sum: value * count,
            min: value,
            max: value,
            values: Vec::new(),
            seen: 0.0,
        });
        let mut left = count.max(0.0).ceil();
        while left >= 1.0 && self.values.len() < MAX_VALUES {
            self.values.push(value);
            self.seen += 1.0;
            left -= 1.0;
        }
        if left < 1.0 {
            return;
        }
        let mut rng = Rng::new();
        let slot = |rng: &mut Rng| {
            #[allow(clippy::cast_possible_truncation)]
            let at = (rng.next() % MAX_VALUES as u64) as usize;
            at
        };
        if left <= EXACT_DRAWS {
            // Algorithm R: the n-th observation replaces a random entry with probability MAX_VALUES / n.
            while left >= 1.0 {
                self.seen += 1.0;
                #[allow(clippy::cast_precision_loss)]
                let keep = rng.unit() * self.seen < MAX_VALUES as f64;
                if keep && let Some(v) = self.values.get_mut(slot(&mut rng)) {
                    *v = value;
                }
                left -= 1.0;
            }
            return;
        }
        // Many observations of one value at once (`Counts`): as many random entries as Algorithm R would replace on average.
        #[allow(clippy::cast_precision_loss)]
        let expected = MAX_VALUES as f64 * ((self.seen + left) / self.seen).ln();
        let mut n = expected.floor() + f64::from(u8::from(rng.unit() < expected.fract()));
        while n >= 1.0 {
            if let Some(v) = self.values.get_mut(slot(&mut rng)) {
                *v = value;
            }
            n -= 1.0;
        }
        self.seen += left;
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

/// A small generator for the sampling, seeded once per call from the system's randomness (xorshift64*; the draws need to be
/// uniform, not secret).
struct Rng(u64);

impl Rng {
    fn new() -> Self {
        let seed = aws_seal::random_bytes::<8>().map_or(0x9E37_79B9_7F4A_7C15, u64::from_le_bytes);
        Self(seed | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number in `0..1`.
    fn unit(&mut self) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let x = (self.next() >> 11) as f64 / (1u64 << 53) as f64;
        x
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
