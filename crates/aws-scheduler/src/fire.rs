//! Firing: the actor works out which schedules are due, records their next fire, deletes the ones that completed with
//! `ActionAfterCompletion: DELETE`, and hands back the invocations, which are made off the actor (`deliver`).
//!
//! The next fire of every schedule is kept on its record, so a region that was reaped and opened again for its timer, or a process
//! that restarted, knows what was due. After a restart a recurring schedule does not replay the fires it missed (AWS does not);
//! a one-time schedule that came due while the process was down fires then, once.

use aws_wire::ApiError;
use serde_json::Value;

use crate::core::Core;
use crate::model::{Schedule, key};
use crate::req::no_schedule;

/// One invocation of a schedule's target.
#[derive(Clone, Debug)]
pub(crate) struct Invocation {
    pub(crate) schedule_arn: String,
    pub(crate) account: String,
    pub(crate) region: String,
    /// The target as stored (with its retry policy and dead-letter queue).
    pub(crate) target: Value,
    /// The time it was scheduled for: `<aws.scheduler.scheduled-time>`.
    pub(crate) scheduled_ms: i64,
    /// `<aws.scheduler.execution-id>`: sixteen hex digits.
    pub(crate) execution_id: String,
}

impl Invocation {
    fn of(s: &Schedule, region: &str, at: i64) -> Self {
        let id: String = aws_wire::RequestId::new()
            .to_string()
            .chars()
            .filter(char::is_ascii_hexdigit)
            .take(16)
            .collect();
        Self {
            schedule_arn: s.arn.clone(),
            account: s.account.clone(),
            region: region.to_owned(),
            target: serde_json::from_str(&s.target).unwrap_or(Value::Null),
            scheduled_ms: at,
            execution_id: id,
        }
    }
}

impl Core {
    /// The earliest time any schedule fires.
    pub(crate) fn next_due(&self) -> Option<i64> {
        self.schedules.iter().filter_map(|(_, s)| s.next_ms).min()
    }

    /// Fires every schedule whose time has come.
    pub(crate) fn tick(&mut self) {
        let now = self.now_ms();
        for k in self.due_keys(now) {
            self.fire_one(&k, now);
        }
    }

    /// After a restart: one-time schedules that came due fire; recurring ones are planned again from now.
    pub(crate) fn resume(&mut self) {
        let now = self.now_ms();
        for k in self.due_keys(now) {
            let Some(s) = self.schedules.get(&k) else {
                continue;
            };
            if aws_cron::parse_scheduler(&s.expression).is_ok_and(|e| e.is_once()) {
                self.fire_one(&k, now);
            } else {
                let next = crate::plan::next(s, now);
                if let Some(s) = self.schedules.get_mut(&k) {
                    s.next_ms = next;
                }
            }
        }
    }

    fn due_keys(&self, now: i64) -> Vec<String> {
        self.schedules
            .iter()
            .filter(|(_, s)| s.next_ms.is_some_and(|at| at <= now))
            .map(|(k, _)| k.to_owned())
            .collect()
    }

    /// Fires one schedule for its due time, plans its next fire, and deletes it if it is complete and asks to be.
    fn fire_one(&mut self, k: &str, now: i64) {
        let Some(s) = self.schedules.get(k) else {
            return;
        };
        let at = s.next_ms.unwrap_or(now);
        self.fired.push(Invocation::of(s, &self.region, at));
        let next = crate::plan::next(s, now.max(at));
        self.settle(k, next);
    }

    fn settle(&mut self, k: &str, next: Option<i64>) {
        let complete = next.is_none();
        if complete && self.schedules.get(k).is_some_and(|s| s.delete_after) {
            self.schedules.remove(k);
        } else if let Some(s) = self.schedules.get_mut(k) {
            s.next_ms = next;
        }
    }

    /// The local hook: fires a schedule now, enabled or not. A one-time schedule's fire is its one fire (it completes, and is deleted
    /// if it asks to be); a recurring schedule gets one extra invocation and keeps its plan.
    pub(crate) fn fire_now(
        &mut self,
        account: Option<&str>,
        group: &str,
        name: &str,
    ) -> Result<Value, ApiError> {
        let suffix = format!("/{group}/{name}");
        let k = match account {
            Some(a) => Some(key(a, &[group, name])).filter(|k| self.schedules.get(k).is_some()),
            None => self
                .schedules
                .iter()
                .map(|(k, _)| k)
                .find(|k| k.ends_with(&suffix))
                .map(str::to_owned),
        };
        let Some(k) = k else {
            return Err(no_schedule(name));
        };
        let now = self.now_ms();
        let Some(s) = self.schedules.get(&k) else {
            return Err(ApiError::internal("the schedule went away"));
        };
        self.fired.push(Invocation::of(s, &self.region, now));
        if aws_cron::parse_scheduler(&s.expression).is_ok_and(|e| e.is_once()) {
            self.settle(&k, None);
        }
        Ok(Value::Null)
    }
}
