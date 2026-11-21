//! Composite alarms, evaluated. As the model documents `PutCompositeAlarm`: "When this operation creates an alarm, the alarm state is
//! immediately set to `INSUFFICIENT_DATA`. The alarm is then evaluated and its state is set appropriately", and that is "the only time
//! that the alarm can be in `INSUFFICIENT_DATA` state": the rule holding is `ALARM`, not holding is `OK`. A composite is judged again
//! whenever an alarm its rule names changes state, which may change a composite above it in turn; "the evaluation of composite alarms
//! stops if CloudWatch detects a cycle", so one change judges each composite at most once.
//!
//! Not built: `ActionsSuppressor` (kept and shown, never suppresses).

use std::collections::BTreeSet;

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::alarms::{alarm_arn, iso};
use crate::core::Core;
use crate::model::Kind;
use crate::req::{Req, validation};
use crate::rule::{self, Rule};

/// The composites one change may judge, at most (a guard; the visited set already ends a cycle).
const MAX_CASCADE: usize = 10_000;

impl Core {
    pub(crate) fn put_composite_alarm(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let rule = rule::parse(r.s("AlarmRule")).map_err(validation)?;
        let name = r.s("AlarmName");
        if rule.children().iter().any(|c| c == name) {
            // (memory)
            return Err(validation(format!(
                "The composite alarm {name} cannot name itself in its AlarmRule."
            )));
        }
        let out = self.store_alarm(r, Kind::Composite, crate::model::Spec::default())?;
        let now = self.now_ms();
        self.judge_composite(name, None, now);
        Ok(out)
    }

    /// The rule of a composite alarm, if `name` is one and its rule reads.
    fn rule_of(&self, name: &str) -> Option<Rule> {
        let a = self
            .alarms
            .get(name)
            .filter(|a| a.kind == Kind::Composite)?;
        let request: Value = serde_json::from_str(&a.request).ok()?;
        rule::parse(request.get("AlarmRule")?.as_str()?).ok()
    }

    /// Judges a composite alarm and moves it if its rule now says otherwise. `cause` is the alarm whose change led here.
    fn judge_composite(&mut self, name: &str, cause: Option<&str>, now: i64) {
        let Some(rule) = self.rule_of(name) else {
            return;
        };
        let state_of = |n: &str| self.alarms.get(n).map(|a| a.state.clone());
        let to = if rule.holds(&state_of) { "ALARM" } else { "OK" };
        let Some(current) = self.alarms.get(name).map(|a| a.state.clone()) else {
            return;
        };
        if current == to {
            return;
        }
        let (reason, data) = self.composite_reason(cause, now);
        self.move_to(name, to, (&reason, Some(&data)), now, true);
    }

    /// Why a composite moved (memory: the shape of AWS's reason and its `triggeringAlarms`).
    fn composite_reason(&self, cause: Option<&str>, now: i64) -> (String, String) {
        let Some((child, a)) = cause.and_then(|c| self.alarms.get(c).map(|a| (c, a))) else {
            let data = json!({"triggeringAlarms": []}).to_string();
            return ("Initial evaluation of the alarm rule".to_owned(), data);
        };
        let arn = alarm_arn(&self.region, &self.account_hint(), child);
        let data = json!({"triggeringAlarms": [{
            "arn": arn,
            "state": {"value": a.state, "timestamp": iso(a.transitioned_ms)},
        }]})
        .to_string();
        let reason = format!("{arn} transitioned to {} at {}", a.state, iso(now));
        (reason, data)
    }

    /// The composite alarms whose rule names `child`.
    fn parents_of(&self, child: &str) -> Vec<String> {
        let names: Vec<String> = self
            .alarms
            .iter()
            .filter(|(_, a)| a.kind == Kind::Composite)
            .map(|(n, _)| n.to_owned())
            .collect();
        names
            .into_iter()
            .filter(|n| {
                self.rule_of(n)
                    .is_some_and(|r| r.children().iter().any(|c| c == child))
            })
            .collect()
    }

    /// After `name` changed state: judges the composites above it, and the ones above those, each at most once.
    pub(crate) fn cascade(&mut self, name: &str) {
        if self.cascading.is_some() {
            if let Some(queue) = &mut self.cascading {
                queue.push(name.to_owned());
            }
            return;
        }
        self.cascading = Some(vec![name.to_owned()]);
        let mut judged = BTreeSet::new();
        let now = self.now_ms();
        while let Some(child) = self.cascading.as_mut().and_then(Vec::pop) {
            for parent in self.parents_of(&child) {
                if judged.len() >= MAX_CASCADE || !judged.insert(parent.clone()) {
                    continue;
                }
                self.judge_composite(&parent, Some(&child), now);
            }
        }
        self.cascading = None;
    }
}
