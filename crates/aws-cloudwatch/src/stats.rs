//! Reading samples back as statistics: the buckets of a series in a window, folded into periods.

use std::collections::BTreeMap;

use aws_wire::ApiError;
use doze_store::record;

use crate::core::{Core, VERSION};
use crate::model::Bucket;
use crate::put::{BUCKET_HIGH_MS, BUCKET_MS};

/// Rows read from the table at a time.
const CHUNK: usize = 500;

/// What one period of one unit holds.
#[derive(Debug, Default)]
pub(crate) struct Agg {
    pub(crate) bucket: Bucket,
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
        other => percentile(other, &b.values),
    }
}

/// `p99`, `p99.9` over the raw values kept; `None` when the name is not a percentile or no raw value was kept (a statistic set has
/// none).
fn percentile(stat: &str, values: &[f64]) -> Option<f64> {
    let p: f64 = stat.strip_prefix('p')?.parse().ok()?;
    if !(0.0..=100.0).contains(&p) || values.is_empty() {
        return None;
    }
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let rank = ((p / 100.0 * v.len() as f64).ceil() as usize).max(1) - 1;
    v.get(rank.min(v.len() - 1)).copied()
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
    /// per unit (`only` keeps one unit).
    pub(crate) fn fold(
        &self,
        no: u32,
        window: (i64, i64),
        period_ms: i64,
        only: Option<usize>,
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
                    let bucket: Bucket = record::decode("bucket", VERSION, value)
                        .map_err(|e| ApiError::internal(e.to_string()))?;
                    let slot = start_ms + (at - start_ms) / period_ms * period_ms;
                    let agg = out.entry((slot, unit)).or_default();
                    agg.bucket.merge(&bucket);
                    agg.bucket
                        .values
                        .extend(bucket.values.iter().take(crate::model::MAX_VALUES));
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

    #[test]
    fn statistics_and_percentiles_are_computed_from_a_bucket() {
        let mut b = Bucket::default();
        for v in 1..=100 {
            b.add(f64::from(v), 1.0);
        }
        let agg = Agg { bucket: b };
        assert_eq!(value_of(&agg, "SampleCount"), Some(100.0));
        assert_eq!(value_of(&agg, "Sum"), Some(5050.0));
        assert_eq!(value_of(&agg, "Average"), Some(50.5));
        assert_eq!(value_of(&agg, "Minimum"), Some(1.0));
        assert_eq!(value_of(&agg, "Maximum"), Some(100.0));
        assert_eq!(value_of(&agg, "p50"), Some(50.0));
        assert_eq!(value_of(&agg, "p99"), Some(99.0));
        assert_eq!(value_of(&agg, "p100"), Some(100.0));
        assert_eq!(value_of(&agg, "nonsense"), None);
        assert!(known("p99.9") && known("Sum") && !known("x"));
    }
}
