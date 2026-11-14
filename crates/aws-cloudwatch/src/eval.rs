//! Judging an alarm, and knowing when to judge it next.
//!
//! An alarm looks at its last `EvaluationPeriods` periods, takes the statistic of each, and counts the ones that breach. As AWS
//! documents it (alarm-evaluation, read 2026-11-13): "For any period of one minute or longer, an alarm is evaluated every minute and
//! the evaluation is based on the window of time defined by the Period and Evaluation Periods. For example, if the Period is 5
//! minutes (300 seconds) and Evaluation Periods is 1, then at the end of minute 5 the alarm evaluates based on data from minutes 1 to
//! 5", and a 10, 20 or 30 second alarm is evaluated every ten seconds; the window slides by that step and is not aligned to the
//! clock. So the window ends at the last step boundary: the minute (or ten seconds) still filling is never judged, and an alarm
//! cannot flip on half a period's data. (A period under ten seconds, which doze accepts and AWS does not, steps by its own length.)
//!
//! That verdict only changes when data arrives or when time moves a step of data from one period of the window to the next, so the
//! next time worth looking at is computed, not polled for: the earliest such instant at which the verdict, with no further data,
//! would differ from the state (see [`Core::refresh_due`]). The service sleeps to the earliest such time over all alarms and to
//! nothing else.
//!
//! How the verdict is reached (breaching count against `DatapointsToAlarm`, missing data by `TreatMissingData`) is from AWS's
//! documentation and memory; the scenarios record the shape of an alarm and its forced state, not an evaluation. Not matched: a
//! multi-day alarm, which AWS evaluates hourly on data up to the hour, is evaluated here like any other.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;

use crate::alarms::iso;
use crate::core::{Core, Outgoing};
use crate::model::{Alarm, Item, Kind, MAX_HISTORY, Spec, UNITS, series_key};
use crate::stats::{Agg, is_percentile, value_of};

/// Instants looked at when working out an alarm's next deadline, at most.
const MAX_CANDIDATES: usize = 10_000;

/// A verdict: the state the data calls for (`None` when the alarm should keep its state), and why.
pub(crate) struct Verdict {
    pub(crate) state: Option<&'static str>,
    pub(crate) reason: String,
    pub(crate) data: String,
}

/// How far the window moves between evaluations: a minute for a period of a minute or more, ten seconds for 10 to 59 seconds, and
/// the period itself below that.
fn step_of(period_ms: i64) -> i64 {
    if period_ms >= 60_000 {
        60_000
    } else if period_ms >= 10_000 {
        10_000
    } else {
        period_ms.max(1)
    }
}

/// The end of the window judged at `at_ms`: the last step boundary at or before it.
fn window_end(at_ms: i64, period_ms: i64) -> i64 {
    at_ms - at_ms.rem_euclid(step_of(period_ms))
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

/// The metric's data by step: what the windows of several instants are made from.
pub(crate) type Steps = BTreeMap<i64, Agg>;

/// The statistic of each period of the window that ends at `end`, from the data by step.
fn periods(spec: &Spec, steps: &Steps, end: i64) -> Window {
    let period = spec.period_s * 1000;
    let mut out = Window::new();
    for k in 0..spec.evaluation_periods {
        let start = end - (k + 1) * period;
        let mut agg: Option<Agg> = None;
        for (_, a) in steps.range(start..start + period) {
            agg.get_or_insert_with(Agg::default).merge(a);
        }
        if let Some(v) = agg.and_then(|a| value_of(&a, &spec.stat)) {
            out.insert(start, v);
        }
    }
    out
}

impl Core {
    /// The alarm's metric from `from` to `to`, by step (all units together unless the alarm names one).
    fn steps(&self, spec: &Spec, from: i64, to: i64) -> Steps {
        let step = step_of(spec.period_s * 1000);
        let from = from - from.rem_euclid(step);
        let key = series_key(&spec.namespace, &spec.metric, &spec.dims);
        let Some(series) = self.series.get(&key) else {
            return Steps::new();
        };
        let unit = spec
            .unit
            .as_deref()
            .and_then(|u| UNITS.iter().position(|x| *x == u));
        let Ok(folded) = self.fold(series.no, (from, to), step, unit, is_percentile(&spec.stat))
        else {
            return Steps::new();
        };
        let mut out = Steps::new();
        for ((at, _), agg) in folded {
            out.entry(at).or_default().merge(&agg);
        }
        out
    }

    /// The statistic of each period of the window an evaluation at `at_ms` looks at.
    pub(crate) fn window(&self, spec: &Spec, at_ms: i64) -> Window {
        let period = spec.period_s * 1000;
        let end = window_end(at_ms, period);
        let steps = self.steps(spec, end - spec.evaluation_periods * period, end);
        periods(spec, &steps, end)
    }

    /// The verdict on the window that ends at `end`, given its data.
    pub(crate) fn judge(spec: &Spec, window: &Window, end: i64, now_ms: i64) -> Verdict {
        let period = spec.period_s * 1000;
        let newest = end - period;
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
        let end = window_end(now, spec.period_s * 1000);
        let v = Self::judge(&spec, &window, end, now);
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

    /// Works out when `name` next needs judging: the first instant, from now, at which the data it has would call for another state
    /// than it is in. The window only changes when a step of data moves into it, from one of its periods to the next, or out of
    /// it, so those instants are the only ones looked at. `after_now` leaves out the present (a forced state stands until the next
    /// step).
    pub(crate) fn refresh_due(&mut self, name: &str, after_now: bool) {
        self.due.remove(name);
        let Some(a) = self.alarms.get(name).filter(|a| a.kind == Kind::Metric) else {
            return;
        };
        let (spec, current) = (a.spec.clone(), a.state.clone());
        let now = self.now_ms();
        let period = spec.period_s * 1000;
        let step = step_of(period);
        let end = window_end(now, period);
        let n = spec.evaluation_periods;
        let steps = self.steps(&spec, end - n * period, now + crate::put::FUTURE_MS + step);
        let mut at: BTreeSet<i64> = BTreeSet::new();
        if after_now {
            at.insert(end + step);
        } else {
            at.insert(now);
        }
        for d in steps.keys() {
            for k in 0..=n {
                let c = d + step + k * period;
                if c > end {
                    at.insert(c);
                }
            }
        }
        for c in at.into_iter().take(MAX_CANDIDATES) {
            let e = window_end(c, period);
            let v = Self::judge(&spec, &periods(&spec, &steps, e), e, now);
            if v.state.is_some_and(|s| s != current) {
                self.due.insert(name.to_owned(), c);
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
