//! Judging an alarm, and knowing when to judge it next.
//!
//! An alarm looks at its last `EvaluationPeriods` periods (aligned to the epoch, the newest being the one that holds the present),
//! takes the statistic of each, and counts the ones that breach. That count only changes when data arrives or when time moves a
//! period out of the window, so the next time worth looking at is computed, not polled for: the earliest boundary at which the
//! verdict, with no further data, would differ from the state (see [`Core::refresh_due`]). The service sleeps to the earliest such
//! time over all alarms and to nothing else.
//!
//! How the verdict is reached (breaching count against `DatapointsToAlarm`, missing data by `TreatMissingData`) is from AWS's
//! documentation and memory; the scenarios record the shape of an alarm and its forced state, not an evaluation.

use std::collections::BTreeMap;

use serde_json::json;

use crate::alarms::iso;
use crate::core::{Core, Outgoing};
use crate::model::{Alarm, Item, Kind, MAX_HISTORY, Spec, UNITS, series_key};
use crate::stats::{Agg, value_of};

/// A verdict: the state the data calls for (`None` when the alarm should keep its state), and why.
pub(crate) struct Verdict {
    pub(crate) state: Option<&'static str>,
    pub(crate) reason: String,
    pub(crate) data: String,
}

/// The start of the period that holds `at_ms`.
fn boundary(at_ms: i64, period_ms: i64) -> i64 {
    at_ms - at_ms.rem_euclid(period_ms)
}

/// Whether `value` breaches the threshold under the alarm's comparison.
fn breaches(spec: &Spec, value: f64) -> bool {
    match spec.comparison.as_str() {
        "GreaterThanOrEqualToThreshold" => value >= spec.threshold,
        "GreaterThanThreshold" => value > spec.threshold,
        "LessThanThreshold" => value < spec.threshold,
        "LessThanOrEqualToThreshold" => value <= spec.threshold,
        _ => false,
    }
}

fn comparison_words(spec: &Spec) -> &'static str {
    match spec.comparison.as_str() {
        "GreaterThanOrEqualToThreshold" => "greater than or equal to",
        "GreaterThanThreshold" => "greater than",
        "LessThanThreshold" => "less than",
        _ => "less than or equal to",
    }
}

/// What judging an alarm reads of the metric: the statistic of each period it looks at, by period start.
pub(crate) type Window = BTreeMap<i64, f64>;

impl Core {
    /// The statistic of each period of the window that ends in the period holding `at_ms`.
    pub(crate) fn window(&self, spec: &Spec, at_ms: i64) -> Window {
        let period = spec.period_s * 1000;
        let end = boundary(at_ms, period) + period;
        let start = end - spec.evaluation_periods * period;
        let key = series_key(&spec.namespace, &spec.metric, &spec.dims);
        let Some(series) = self.series.get(&key) else {
            return Window::new();
        };
        let unit = spec
            .unit
            .as_deref()
            .and_then(|u| UNITS.iter().position(|x| *x == u));
        let Ok(folded) = self.fold(series.no, (start, end), period, unit) else {
            return Window::new();
        };
        let mut per: BTreeMap<i64, Agg> = BTreeMap::new();
        for ((at, _), agg) in folded {
            let slot = per.entry(at).or_default();
            slot.bucket.merge(&agg.bucket);
            slot.bucket.values.extend(agg.bucket.values);
        }
        per.iter()
            .filter_map(|(at, agg)| Some((*at, value_of(agg, &spec.stat)?)))
            .collect()
    }

    /// The verdict at the period that holds `at_ms`, given the data of `window`.
    pub(crate) fn judge(spec: &Spec, window: &Window, at_ms: i64, now_ms: i64) -> Verdict {
        let period = spec.period_s * 1000;
        let newest = boundary(at_ms, period);
        let n = spec.evaluation_periods;
        let (mut breaching, mut fine, mut unknown) = (Vec::new(), 0, 0);
        for k in 0..n {
            let slot = newest - k * period;
            match (window.get(&slot), spec.treat_missing.as_str()) {
                (Some(v), _) if breaches(spec, *v) => breaching.push((*v, slot)),
                (Some(_), _) | (None, "notBreaching") => fine += 1,
                (None, "breaching") => breaching.push((f64::NAN, slot)),
                (None, _) => unknown += 1,
            }
        }
        let m = spec.datapoints_to_alarm;
        let (state, reason) = if i64::try_from(breaching.len()).unwrap_or(0) >= m {
            (Some("ALARM"), crossed(spec, &breaching, n, m, true))
        } else if fine > n - m {
            (Some("OK"), crossed(spec, &breaching, n, m, false))
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
        let data = json!({
            "version": "1.0",
            "queryDate": iso(now_ms),
            "startDate": iso(newest - (n - 1) * period),
            "statistic": spec.stat,
            "period": spec.period_s,
            "recentDatapoints": window.values().rev().take(usize::try_from(n).unwrap_or(1)).rev().collect::<Vec<_>>(),
            "threshold": spec.threshold,
        })
        .to_string();
        Verdict {
            state,
            reason,
            data,
        }
    }

    /// Moves an alarm to `state`. A change is recorded in its history and, when its actions are enabled, queues them.
    pub(crate) fn move_to(
        &mut self,
        name: &str,
        state: &str,
        why: (&str, Option<&str>),
        now: i64,
        changed: bool,
    ) {
        let Some(a) = self.alarms.get_mut(name) else {
            return;
        };
        let old = std::mem::replace(&mut a.state, state.to_owned());
        a.reason = why.0.to_owned();
        a.reason_data = why.1.map(str::to_owned);
        a.state_updated_ms = now;
        if !changed {
            return;
        }
        a.transitioned_ms = now;
        let actions = if !a.actions_enabled {
            Vec::new()
        } else {
            match state {
                "ALARM" => a.alarm_actions.clone(),
                "OK" => a.ok_actions.clone(),
                _ => a.insufficient_actions.clone(),
            }
        };
        let (alarm_type, reason, data) = (
            crate::alarms::type_of(a.kind),
            a.reason.clone(),
            a.reason_data.clone(),
        );
        let history = json!({
            "version": "1.0",
            "oldState": {"stateValue": old},
            "newState": {"stateValue": state, "stateReason": reason, "stateReasonData": data},
        })
        .to_string();
        self.record(
            name,
            now,
            (
                "StateUpdate",
                &format!("Alarm updated from {old} to {state}"),
                &history,
                alarm_type,
            ),
        );
        for action in actions {
            self.outbox.push(Outgoing {
                alarm: name.to_owned(),
                action,
                old: old.clone(),
                new: state.to_owned(),
                at_ms: now,
            });
        }
    }

    /// Adds an entry to an alarm's history, keeping the newest [`MAX_HISTORY`].
    pub(crate) fn record(&mut self, name: &str, at_ms: i64, entry: (&str, &str, &str, &str)) {
        let item = Item {
            at_ms,
            kind: entry.0.to_owned(),
            summary: entry.1.to_owned(),
            data: entry.2.to_owned(),
            alarm_type: entry.3.to_owned(),
        };
        if self.history.get(name).is_none() {
            self.history
                .insert(name.to_owned(), crate::model::History::default());
        }
        if let Some(h) = self.history.get_mut(name) {
            h.items.push(item);
            let excess = h.items.len().saturating_sub(MAX_HISTORY);
            h.items.drain(..excess);
        }
    }

    /// Judges the alarm now and moves it if the data calls for another state.
    fn evaluate(&mut self, name: &str, now: i64) {
        let Some(a) = self.alarms.get(name).filter(|a| a.kind == Kind::Metric) else {
            return;
        };
        let (spec, current) = (a.spec.clone(), a.state.clone());
        let window = self.window(&spec, now);
        let v = Self::judge(&spec, &window, now, now);
        if let Some(state) = v.state.filter(|s| *s != current) {
            self.move_to(name, state, (&v.reason, Some(&v.data)), now, true);
        }
        self.refresh_due(name, false);
    }

    /// The alarms on the metric with this series key.
    fn alarms_on(&self, key: &str) -> Vec<String> {
        self.alarms
            .iter()
            .filter(|(_, a): &(&str, &Alarm)| {
                a.kind == Kind::Metric
                    && series_key(&a.spec.namespace, &a.spec.metric, &a.spec.dims) == key
            })
            .map(|(n, _)| n.to_owned())
            .collect()
    }

    /// Notes the metrics a call wrote, to be judged once the data is where a read finds it (see [`Core::evaluate_pending`]). A metric
    /// no alarm watches is not noted: a write that nothing waits on is not held up for the disk.
    pub(crate) fn note_written(&mut self, keys: Vec<String>) {
        for key in keys {
            if !self.alarms_on(&key).is_empty() {
                self.to_evaluate.push(key);
            }
        }
    }

    /// Judges every alarm on the metrics that were written.
    pub(crate) fn evaluate_pending(&mut self) {
        let now = self.now_ms();
        for key in std::mem::take(&mut self.to_evaluate) {
            for name in self.alarms_on(&key) {
                self.evaluate(&name, now);
            }
        }
    }

    /// Works out when `name` next needs judging: the first period boundary, from now, at which the data it has would call for another
    /// state than it is in. `after_now` leaves out the present (a forced state stands until the next boundary).
    pub(crate) fn refresh_due(&mut self, name: &str, after_now: bool) {
        self.due.remove(name);
        let Some(a) = self.alarms.get(name).filter(|a| a.kind == Kind::Metric) else {
            return;
        };
        let (spec, current) = (a.spec.clone(), a.state.clone());
        let now = self.now_ms();
        let window = self.window(&spec, now);
        let period = spec.period_s * 1000;
        let here = boundary(now, period);
        for j in i64::from(after_now)..=spec.evaluation_periods + 1 {
            let at = if j == 0 { now } else { here + j * period };
            let v = Self::judge(&spec, &window, at, now);
            if v.state.is_some_and(|s| s != current) {
                self.due.insert(name.to_owned(), at);
                return;
            }
        }
    }

    /// Works out the deadlines of every alarm, once after the service opens.
    pub(crate) fn ensure_due(&mut self) {
        if self.due_ready {
            return;
        }
        self.due_ready = true;
        let names: Vec<String> = self.alarms.iter().map(|(n, _)| n.to_owned()).collect();
        for n in names {
            self.refresh_due(&n, false);
        }
    }

    /// The earliest time an alarm needs judging.
    pub(crate) fn earliest_due(&self) -> Option<i64> {
        self.due.values().min().copied()
    }

    /// Judges the alarms whose time has come.
    pub(crate) fn tick(&mut self) {
        let now = self.now_ms();
        let ready: Vec<String> = self
            .due
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(n, _)| n.clone())
            .collect();
        for name in ready {
            self.evaluate(&name, now);
        }
    }
}

/// The reason of a verdict that crossed, or came back, in AWS's words.
fn crossed(spec: &Spec, breaching: &[(f64, i64)], n: i64, m: i64, alarm: bool) -> String {
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
    if alarm {
        format!(
            "Threshold Crossed: {} out of the last {n} datapoints [{}] was {} the threshold ({:?}) (minimum {m} {s} for OK -> ALARM transition).",
            breaching.len(),
            shown.join(", "),
            comparison_words(spec),
            spec.threshold
        )
    } else {
        format!(
            "Threshold Crossed: {n} datapoints were not {} the threshold ({:?}) (minimum {m} {s} for ALARM -> OK transition).",
            comparison_words(spec),
            spec.threshold
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
