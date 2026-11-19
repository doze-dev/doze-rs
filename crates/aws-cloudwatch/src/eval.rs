//! Knowing when to judge an alarm, and judging it then (the verdict itself is `judge.rs`).
//!
//! An alarm looks at its last `EvaluationPeriods` periods, takes the statistic of each, and counts the ones that breach. As AWS
//! documents it (alarm-evaluation, read 2026-11-13): "For any period of one minute or longer, an alarm is evaluated every minute and
//! the evaluation is based on the window of time defined by the Period and Evaluation Periods. For example, if the Period is 5
//! minutes (300 seconds) and Evaluation Periods is 1, then at the end of minute 5 the alarm evaluates based on data from minutes 1 to
//! 5", and a 10, 20 or 30 second alarm is evaluated every ten seconds; the window slides by that step and is not aligned to the
//! clock. So the window ends at the last step boundary: the minute (or ten seconds) still filling is never judged, and an alarm
//! cannot flip on half a period's data. A period AWS refuses (under ten seconds, 45 seconds) is refused when the alarm is put
//! (`period.rs`).
//!
//! That verdict only changes when data arrives or when time moves a step of data from one period of the window to the next, so the
//! next time worth looking at is computed, not polled for: the earliest such instant at which the verdict, with no further data,
//! would differ from the state (see [`Core::refresh_due`]). The service sleeps to the earliest such time over all alarms and to
//! nothing else. A metric math alarm is judged the same way on the value of its expression, its deadlines worked out from the data
//! of every metric it reads (`math_alarm.rs`).
//!
//! A log alarm's deadline is its next scheduled query (`log_eval`); a composite alarm has none, being judged when an alarm it names
//! changes (`composite`).
//!
//! Not matched: a multi-day alarm, which AWS evaluates hourly on data up to the hour, is evaluated here like any other.

use std::collections::{BTreeMap, BTreeSet};

use crate::core::Core;
use crate::judge::{Verdict, Window, judge};
use crate::math_alarm::MathAlarm;
use crate::model::{Kind, Spec, UNITS, series_key};
use crate::stats::{Agg, is_percentile, value_of};

/// Instants looked at when working out an alarm's next deadline, at most.
const MAX_CANDIDATES: usize = 10_000;

/// The metric's data by step: what the windows of several instants are made from.
pub(crate) type Steps = BTreeMap<i64, Agg>;

/// How far the window moves between evaluations: a minute for a period of a minute or more, ten seconds for 10, 20 or 30 seconds
/// (the only shorter periods an alarm may have; the last branch only keeps a stored record from looping).
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

/// The statistic of each of the `spec.evaluation_periods` periods before `end`, from the data by step.
pub(crate) fn periods(spec: &Spec, steps: &Steps, end: i64) -> Window {
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

/// What an alarm is judged on: how (`spec`), the metrics it reads (one spec each, whose `evaluation_periods` say how far back it is
/// read), and its expressions when it is a metric math alarm.
pub(crate) struct Plan {
    pub(crate) spec: Spec,
    pub(crate) inputs: Vec<Spec>,
    pub(crate) math: Option<MathAlarm>,
}

impl Plan {
    /// How far back from the window's end the metrics are read.
    fn span_ms(&self) -> i64 {
        let period = self.spec.period_s * 1000;
        self.inputs
            .iter()
            .map(|i| i.evaluation_periods)
            .max()
            .unwrap_or(0)
            * period
    }

    fn verdict(&self, steps: &[Steps], end: i64, now: i64) -> Verdict {
        match &self.math {
            Some(m) => m.verdict(self, steps, end, now),
            None => {
                let window = steps
                    .first()
                    .map(|s| periods(&self.spec, s, end))
                    .unwrap_or_default();
                judge(&self.spec, &window, None, end, now)
            }
        }
    }
}

impl Core {
    /// One metric from `from` to `to`, by step (all units together unless the spec names one).
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

    /// What the alarm called `name` is judged on, when it is judged here.
    fn plan(&self, name: &str) -> Option<Plan> {
        let a = self.alarms.get(name)?;
        match a.kind {
            Kind::Metric => Some(Plan {
                spec: a.spec.clone(),
                inputs: vec![a.spec.clone()],
                math: None,
            }),
            Kind::Math => crate::math_alarm::plan(a),
            Kind::Composite | Kind::Log => None,
        }
    }

    fn input_steps(&self, plan: &Plan, from: i64, to: i64) -> Vec<Steps> {
        plan.inputs
            .iter()
            .map(|i| self.steps(i, from, to))
            .collect()
    }

    /// Judges the alarm now and moves it if the data calls for another state.
    fn evaluate(&mut self, name: &str, now: i64) {
        let (Some(plan), Some(current)) = (
            self.plan(name),
            self.alarms.get(name).map(|a| a.state.clone()),
        ) else {
            return;
        };
        let end = window_end(now, plan.spec.period_s * 1000);
        let steps = self.input_steps(&plan, end - plan.span_ms(), end);
        let v = plan.verdict(&steps, end, now);
        if let Some(state) = v.state.filter(|s| *s != current) {
            self.move_to(name, state, (&v.reason, Some(&v.data)), now, true);
        }
        self.refresh_due(name, false);
    }

    /// The alarms that read the metric with this series key.
    fn alarms_on(&self, key: &str) -> Vec<String> {
        let names: Vec<String> = self.alarms.iter().map(|(n, _)| n.to_owned()).collect();
        names
            .into_iter()
            .filter(|n| {
                self.plan(n).is_some_and(|p| {
                    p.inputs
                        .iter()
                        .any(|i| series_key(&i.namespace, &i.metric, &i.dims) == key)
                })
            })
            .collect()
    }

    /// The namespaces the alarms judged here read.
    pub(crate) fn watched_namespaces(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .alarms
            .iter()
            .filter_map(|(n, _)| self.plan(n))
            .flat_map(|p| p.inputs.into_iter().map(|i| i.namespace))
            .collect();
        out.sort();
        out.dedup();
        out
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
        // A log alarm is judged on its schedule, not on data (`log_eval`).
        if let Some(at) = self.log_due(name) {
            self.due.insert(name.to_owned(), at);
            return;
        }
        let (Some(plan), Some(current)) = (
            self.plan(name),
            self.alarms.get(name).map(|a| a.state.clone()),
        ) else {
            return;
        };
        let now = self.now_ms();
        let period = plan.spec.period_s * 1000;
        let step = step_of(period);
        let end = window_end(now, period);
        let span = plan.span_ms();
        let steps = self.input_steps(&plan, end - span, now + crate::put::FUTURE_MS + step);
        let mut at: BTreeSet<i64> = BTreeSet::new();
        at.insert(if after_now { end + step } else { now });
        let reach = span / period.max(1);
        for d in steps.iter().flat_map(BTreeMap::keys) {
            for k in 0..=reach {
                let c = d + step + k * period;
                if c > end {
                    at.insert(c);
                }
            }
        }
        for c in at.into_iter().take(MAX_CANDIDATES) {
            let v = plan.verdict(&steps, window_end(c, period), now);
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
            if self.alarms.get(&name).is_some_and(|a| a.kind == Kind::Log) {
                self.start_log_run(&name, now);
            } else {
                self.evaluate(&name, now);
            }
        }
    }
}
