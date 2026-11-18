//! The periods an alarm may have, from the model's documentation of `PutMetricAlarm`'s `Period`: "Valid values are 10, 20, 30, and
//! any multiple of 60", and "`Period` multiplied by `EvaluationPeriods` can't be more than 604,800 seconds. For alarms with a period of
//! less than one hour (3,600 seconds), the total evaluation period can't be longer than one day (86,400 seconds)." A metric math
//! alarm's periods are those of its `MetricStat` queries. The refusals are `ValidationError`, as the recorded refusals of
//! `PutMetricAlarm` are; their words are from memory.

use aws_wire::ApiError;
use serde_json::Value;

use crate::req::{Req, validation};

/// Whether `period` (seconds) is one an alarm may have.
pub(crate) fn valid(period: i64) -> bool {
    matches!(period, 10 | 20 | 30) || (period > 0 && period % 60 == 0)
}

/// The longest window (`Period` × `EvaluationPeriods`, seconds) an alarm with `period` may look at.
fn longest_window(period: i64) -> i64 {
    if period < 3_600 { 86_400 } else { 604_800 }
}

/// Refuses an alarm whose period, or window, AWS refuses.
///
/// # Errors
/// `ValidationError` naming the parameter.
pub(crate) fn check(r: &Req<'_>, math: bool) -> Result<(), ApiError> {
    let periods: Vec<(String, i64)> = if math {
        r.v.get("Metrics")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
            .filter_map(|(i, m)| {
                let p = m.pointer("/MetricStat/Period")?.as_i64()?;
                Some((format!("Metrics.member.{}.MetricStat.Period", i + 1), p))
            })
            .collect()
    } else {
        r.int("Period")
            .map(|p| ("Period".to_owned(), p))
            .into_iter()
            .collect()
    };
    for (field, period) in &periods {
        if !valid(*period) {
            // (memory)
            return Err(validation(format!(
                "The parameter {field} must be 10, 20, 30 or a multiple of 60 (it is {period})."
            )));
        }
    }
    if let (false, Some(period), Some(n)) = (math, r.int("Period"), r.int("EvaluationPeriods")) {
        let window = period.saturating_mul(n);
        let longest = longest_window(period);
        if window > longest {
            // (memory)
            return Err(validation(format!(
                "Period multiplied by EvaluationPeriods must not be more than {longest} seconds (it is {window})."
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_valid_periods_are_ten_twenty_thirty_and_whole_minutes() {
        for ok in [10, 20, 30, 60, 120, 300, 3_600, 86_400] {
            assert!(valid(ok), "{ok}");
        }
        for bad in [0, -60, 1, 5, 9, 15, 45, 61, 90] {
            assert!(!valid(bad), "{bad}");
        }
        assert_eq!(longest_window(60), 86_400);
        assert_eq!(longest_window(3_600), 604_800);
    }
}
