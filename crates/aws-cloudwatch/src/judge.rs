//! The verdict on an alarm's window: how many of its periods breach, against `DatapointsToAlarm`, with missing data judged by
//! `TreatMissingData`, and the reason in AWS's words. A metric alarm and a metric math alarm are judged the same way; an alarm on an
//! anomaly detection band (`ThresholdMetricId`) compares each value with the band at that period instead of with `Threshold`.
//!
//! The verdict rules are from AWS's documentation and memory; the reasons of a threshold alarm follow AWS's wording (memory: the
//! scenarios record an alarm's shape and its forced state, not an evaluation). The reason of a band alarm is (memory).

use std::collections::BTreeMap;

use serde_json::json;

use crate::alarms::iso;
use crate::model::Spec;

/// A verdict: the state the data calls for (`None` when the alarm should keep its state), and why.
pub(crate) struct Verdict {
    pub(crate) state: Option<&'static str>,
    pub(crate) reason: String,
    pub(crate) data: String,
}

/// What judging an alarm reads: the value of each period it looks at, by period start.
pub(crate) type Window = BTreeMap<i64, f64>;

/// An anomaly detection band, by period start: lower and upper edge.
pub(crate) type Bands = BTreeMap<i64, (f64, f64)>;

/// Whether `value` breaches under the alarm's comparison (against the band at that period, for a band alarm).
fn breaches(spec: &Spec, value: f64, band: Option<(f64, f64)>) -> bool {
    match (spec.comparison.as_str(), band) {
        ("GreaterThanOrEqualToThreshold", _) => value >= spec.threshold,
        ("GreaterThanThreshold", _) => value > spec.threshold,
        ("LessThanThreshold", _) => value < spec.threshold,
        ("LessThanOrEqualToThreshold", _) => value <= spec.threshold,
        ("LessThanLowerOrGreaterThanUpperThreshold", Some((lo, hi))) => value < lo || value > hi,
        ("LessThanLowerThreshold", Some((lo, _))) => value < lo,
        ("GreaterThanUpperThreshold", Some((_, hi))) => value > hi,
        _ => false,
    }
}

fn comparison_words(spec: &Spec) -> &'static str {
    match spec.comparison.as_str() {
        "GreaterThanOrEqualToThreshold" => "greater than or equal to",
        "GreaterThanThreshold" => "greater than",
        "LessThanThreshold" => "less than",
        "LessThanOrEqualToThreshold" => "less than or equal to",
        "LessThanLowerThreshold" => "less than the lower edge of",
        "GreaterThanUpperThreshold" => "greater than the upper edge of",
        _ => "outside",
    }
}

/// The verdict on the window that ends at `end`, given its data.
pub(crate) fn judge(
    spec: &Spec,
    window: &Window,
    bands: Option<&Bands>,
    end: i64,
    now_ms: i64,
) -> Verdict {
    let period = spec.period_s * 1000;
    let newest = end - period;
    let n = spec.evaluation_periods;
    let (mut breaching, mut fine, mut unknown) = (Vec::new(), 0, 0);
    for k in 0..n {
        let slot = newest - k * period;
        let band = bands.and_then(|b| b.get(&slot).copied());
        match (window.get(&slot), spec.treat_missing.as_str()) {
            (Some(v), _) if breaches(spec, *v, band) => breaching.push((*v, slot)),
            (Some(_), _) | (None, "notBreaching") => fine += 1,
            (None, "breaching") => breaching.push((f64::NAN, slot)),
            (None, _) => unknown += 1,
        }
    }
    let m = spec.datapoints_to_alarm;
    let banded = bands.is_some();
    let (state, reason) = if i64::try_from(breaching.len()).unwrap_or(0) >= m {
        (
            Some("ALARM"),
            crossed(spec, &breaching, (n, m), true, banded),
        )
    } else if fine > n - m {
        (Some("OK"), crossed(spec, &breaching, (n, m), false, banded))
    } else if spec.treat_missing == "ignore" {
        (None, String::new())
    } else {
        let s = if unknown == 1 {
            "datapoint was"
        } else {
            "datapoints were"
        };
        (
            Some("INSUFFICIENT_DATA"),
            format!("Insufficient Data: {unknown} {s} unknown."),
        )
    };
    let mut data = json!({
        "version": "1.0",
        "queryDate": iso(now_ms),
        "startDate": iso(newest - (n - 1) * period),
        "period": spec.period_s,
        "recentDatapoints": window.values().rev().take(usize::try_from(n).unwrap_or(1)).rev().collect::<Vec<_>>(),
    });
    if let serde_json::Value::Object(o) = &mut data {
        if !spec.stat.is_empty() {
            o.insert("statistic".to_owned(), json!(spec.stat));
        }
        if !banded {
            o.insert("threshold".to_owned(), json!(spec.threshold));
        }
    }
    Verdict {
        state,
        reason,
        data: data.to_string(),
    }
}

/// The reason of a verdict that crossed, or came back, in AWS's words.
fn crossed(
    spec: &Spec,
    breaching: &[(f64, i64)],
    (n, m): (i64, i64),
    alarm: bool,
    banded: bool,
) -> String {
    let shown: Vec<String> = breaching
        .iter()
        .map(|(v, at)| {
            if v.is_nan() {
                format!("missing ({})", stamp(*at))
            } else {
                format!("{v:?} ({})", stamp(*at))
            }
        })
        .collect();
    let s = if m == 1 { "datapoint" } else { "datapoints" };
    let what = if banded {
        format!("{} the band", comparison_words(spec))
    } else {
        format!(
            "{} the threshold ({:?})",
            comparison_words(spec),
            spec.threshold
        )
    };
    if alarm {
        format!(
            "Threshold Crossed: {} out of the last {n} datapoints [{}] was {what} (minimum {m} {s} for OK -> ALARM transition).",
            breaching.len(),
            shown.join(", "),
        )
    } else {
        format!(
            "Threshold Crossed: {n} datapoints were not {what} (minimum {m} {s} for ALARM -> OK transition)."
        )
    }
}

/// A time as AWS prints it in a reason: `12/11/26 10:00:00`.
fn stamp(ms: i64) -> String {
    let iso = aws_core::time::format_iso8601(ms.div_euclid(1000));
    match (
        iso.get(2..4),
        iso.get(5..7),
        iso.get(8..10),
        iso.get(11..19),
    ) {
        (Some(y), Some(mo), Some(d), Some(t)) => format!("{d}/{mo}/{y} {t}"),
        _ => iso,
    }
}
