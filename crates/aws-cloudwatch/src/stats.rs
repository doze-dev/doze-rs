//! Reading samples back as statistics: the buckets of a series in a window, folded into periods.

use std::collections::BTreeMap;

use aws_wire::ApiError;

use crate::core::Core;
use crate::model::Bucket;
use crate::put::{BUCKET_HIGH_MS, BUCKET_MS};

/// Rows read from the table at a time.
const CHUNK: usize = 500;

/// What one period of one unit holds: the statistics, and, when a percentile is asked for, the raw values each with the number of
/// observations it stands for (one while a bucket kept every value; more for a bucket that kept a sample).
#[derive(Debug, Default, Clone)]
pub(crate) struct Agg {
    pub(crate) bucket: Bucket,
    pub(crate) samples: Vec<(f64, f64)>,
}

impl Agg {
    /// Adds a stored bucket; its values are kept only when `samples` is wanted.
    fn absorb(&mut self, b: &Bucket, samples: bool) {
        self.bucket.merge(b);
        if samples && !b.values.is_empty() {
            #[allow(clippy::cast_precision_loss)]
            let weight = (b.seen / b.values.len() as f64).max(1.0);
            self.samples.extend(b.values.iter().map(|v| (*v, weight)));
        }
    }

    /// Adds another period's (or unit's) aggregate.
    pub(crate) fn merge(&mut self, other: &Self) {
        self.bucket.merge(&other.bucket);
        self.samples.extend_from_slice(&other.samples);
    }
}

/// The value of a statistic over a period: one of the five, or a percentile such as `p99`.
pub(crate) fn value_of(agg: &Agg, stat: &str) -> Option<f64> {
    let b = &agg.bucket;
    match stat {
        "SampleCount" => Some(b.count),
        "Sum" => Some(b.sum),
        "Minimum" => Some(b.min),
        "Maximum" => Some(b.max),
        "Average" => (b.count > 0.0).then(|| b.sum / b.count),
        other => percentile(other, &agg.samples),
    }
}

/// Whether `stat` is a percentile (and so needs the raw values).
pub(crate) fn is_percentile(stat: &str) -> bool {
    stat.starts_with('p') && known(stat)
}

/// `p99`, `p99.9` over the values kept, by nearest rank: the smallest value at or below which `p` per cent of the observations lie,
/// each value counting for the observations it stands for. `None` when the name is not a percentile or no value was kept (a
/// statistic set has none). Nearest rank is from memory: AWS documents its percentiles as approximations and records none here.
fn percentile(stat: &str, samples: &[(f64, f64)]) -> Option<f64> {
    let p: f64 = stat.strip_prefix('p')?.parse().ok()?;
    if !(0.0..=100.0).contains(&p) || samples.is_empty() {
        return None;
    }
    let mut v = samples.to_vec();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: f64 = v.iter().map(|(_, w)| w).sum();
    let target = p / 100.0 * total;
    let mut seen = 0.0;
    for (value, weight) in &v {
        seen += weight;
        if seen + 1e-9 >= target {
            return Some(*value);
        }
    }
    v.last().map(|(value, _)| *value)
}

/// Whether `stat` names a statistic this service computes.
pub(crate) fn known(stat: &str) -> bool {
    matches!(
        stat,
        "SampleCount" | "Sum" | "Minimum" | "Maximum" | "Average"
    ) || stat
        .strip_prefix('p')
        .and_then(|p| p.parse::<f64>().ok())
        .is_some_and(|p| (0.0..=100.0).contains(&p))
}

impl Core {
    /// The buckets of series `no` with their start in `start_ms..end_ms`, folded into periods of `period_ms` counted from the start,
    /// per unit (`only` keeps one unit). `samples` keeps the raw values, for a percentile.
    pub(crate) fn fold(
        &self,
        no: u32,
        window: (i64, i64),
        period_ms: i64,
        only: Option<usize>,
        samples: bool,
    ) -> Result<BTreeMap<(i64, usize), Agg>, ApiError> {
        let (start_ms, end_ms) = window;
        let mut out: BTreeMap<(i64, usize), Agg> = BTreeMap::new();
        for high_res in [false, true] {
            let width = if high_res { BUCKET_HIGH_MS } else { BUCKET_MS };
            let first = start_ms - start_ms.rem_euclid(width);
            let mut from = crate::model::sample_key(no, high_res, first, 0);
            let to = crate::model::sample_key(no, high_res, end_ms, 0);
            loop {
                let rows = self
                    .samples
                    .scan(self.db.as_ref(), &from, &to, CHUNK, false)
                    .map_err(|e| ApiError::internal(e.to_string()))?;
                let Some((last, _)) = rows.last() else {
                    break;
                };
                from = format!("{last}\u{0}");
                let n = rows.len();
                for (key, value) in &rows {
                    let (Some(at), Some(unit)) = (
                        key.get(9..25).and_then(|h| i64::from_str_radix(h, 16).ok()),
                        key.get(25..27)
                            .and_then(|h| usize::from_str_radix(h, 16).ok()),
                    ) else {
                        continue;
                    };
                    if at < start_ms || only.is_some_and(|u| u != unit) {
                        continue;
                    }
                    let bucket =
                        Bucket::decode(value).map_err(|e| ApiError::internal(e.to_string()))?;
                    let slot = start_ms + (at - start_ms) / period_ms * period_ms;
                    out.entry((slot, unit))
                        .or_default()
                        .absorb(&bucket, samples);
                }
                if n < CHUNK {
                    break;
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agg_of(b: &Bucket) -> Agg {
        let mut a = Agg::default();
        a.absorb(b, true);
        a
    }

    #[test]
    fn statistics_and_percentiles_are_computed_from_a_bucket() {
        let mut b = Bucket::default();
        for v in 1..=100 {
            b.add(f64::from(v), 1.0);
        }
        let agg = agg_of(&b);
        assert_eq!(value_of(&agg, "SampleCount"), Some(100.0));
        assert_eq!(value_of(&agg, "Sum"), Some(5050.0));
        assert_eq!(value_of(&agg, "Average"), Some(50.5));
        assert_eq!(value_of(&agg, "Minimum"), Some(1.0));
        assert_eq!(value_of(&agg, "Maximum"), Some(100.0));
        assert_eq!(value_of(&agg, "p50"), Some(50.0));
        assert_eq!(value_of(&agg, "p99"), Some(99.0));
        assert_eq!(value_of(&agg, "p100"), Some(100.0));
        assert_eq!(value_of(&agg, "p0"), Some(1.0));
        assert_eq!(value_of(&agg, "nonsense"), None);
        assert!(known("p99.9") && known("Sum") && !known("x"));
    }

    #[test]
    fn percentiles_are_exact_up_to_the_sample_size_and_close_past_it() {
        // Exact while every value is kept: 1..=1024 in a scrambled order.
        let mut b = Bucket::default();
        for i in 0..1024u32 {
            b.add(f64::from((i * 389) % 1024 + 1), 1.0);
        }
        assert_eq!(b.values.len(), crate::model::MAX_VALUES);
        let agg = agg_of(&b);
        assert_eq!(value_of(&agg, "p50"), Some(512.0));
        assert_eq!(value_of(&agg, "p99"), Some(1014.0));
        // Under load: 100,000 observations, the first 128 of them huge. The old cap kept only those and called p50 huge; the
        // sample keeps the distribution, and the percentile is within a few per cent of the true rank.
        let mut b = Bucket::default();
        for i in 0..100_000u32 {
            let v = if i < 128 {
                1_000_000.0
            } else {
                f64::from(i % 1000)
            };
            b.add(v, 1.0);
        }
        assert_eq!(b.values.len(), crate::model::MAX_VALUES);
        assert_eq!(b.seen, 100_000.0);
        let agg = agg_of(&b);
        let p50 = value_of(&agg, "p50").unwrap_or(f64::NAN);
        assert!((430.0..=570.0).contains(&p50), "p50 was {p50}");
        let p90 = value_of(&agg, "p90").unwrap_or(f64::NAN);
        assert!((850.0..=950.0).contains(&p90), "p90 was {p90}");
        // A value with a large count is sampled by its weight, not one draw at a time.
        let mut b = Bucket::default();
        b.add(1.0, 2000.0);
        b.add(9.0, 1_000_000.0);
        assert_eq!(b.seen, 1_002_000.0);
        let agg = agg_of(&b);
        assert_eq!(value_of(&agg, "p50"), Some(9.0));
        assert_eq!(value_of(&agg, "SampleCount"), Some(1_002_000.0));
    }

    #[test]
    fn buckets_of_different_sizes_count_by_what_they_stand_for() {
        // One bucket kept every value (ten 1s); another kept a sample of 1024 of 100,000 observations of 5. p50 is 5, not split by
        // the number of kept entries.
        let mut small = Bucket::default();
        small.add(1.0, 10.0);
        let mut big = Bucket::default();
        for _ in 0..100_000 {
            big.add(5.0, 1.0);
        }
        let mut agg = agg_of(&small);
        agg.absorb(&big, true);
        assert_eq!(value_of(&agg, "p50"), Some(5.0));
        assert_eq!(value_of(&agg, "p0.001"), Some(1.0));
    }

    #[test]
    fn a_bucket_of_the_first_version_is_read() {
        #[derive(serde::Serialize)]
        struct V1 {
            count: f64,
            sum: f64,
            min: f64,
            max: f64,
            values: Vec<f64>,
        }
        let bytes = doze_store::record::encode(
            "bucket",
            1,
            &V1 {
                count: 2.0,
                sum: 3.0,
                min: 1.0,
                max: 2.0,
                values: vec![1.0, 2.0],
            },
        )
        .unwrap_or_default();
        let b = Bucket::decode(&bytes).unwrap_or_default();
        assert_eq!((b.count, b.seen, b.values.len()), (2.0, 2.0, 2));
    }
}
