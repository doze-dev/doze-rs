//! Scheduled rotation: when a secret's next rotation window opens, and the timer's tick that starts every rotation that is due.
//!
//! The rules are AWS's (the model's documentation of `RotationRulesType`): `AutomaticallyAfterDays` and a `rate()` expression count
//! from the last rotation and open their window at midnight UTC; a `cron()` expression names the window's start itself; a manual
//! `PutSecretValue` or `UpdateSecret` counts as a rotation for `AutomaticallyAfterDays`. AWS rotates "at any time during the window";
//! here a rotation starts when the window opens, which is a time AWS could have chosen and is the same on every run. `Duration` (the
//! window's length) is stored and shown; it changes nothing locally, because the rotation starts at the window's first instant.
//!
//! Nothing polls: after every call the actor says when the soonest window of the region opens ([`Core::next_due`]), the region's one
//! timer (`doze_runtime::Deadlines`) sleeps until then and sends a tick, and `resume` re-arms it when the process starts.

use aws_cron::Schedule;
use aws_wire::ApiError;
use aws_wire::service::Caller;

use crate::core::{Answer, Core, invalid_parameter};
use crate::model::{DAY_MS, Rotation, day_of};
use crate::rotate::{Run, Step, open_pending};

/// The shortest interval AWS lets a schedule rotate at (the model: "as often as every four hours").
const SHORTEST_MS: i64 = 4 * 3_600_000;

impl Rotation {
    /// When the next rotation window opens: what `NextRotationDate` shows and when the timer starts the rotation. `None` while
    /// rotation is off, a rotation is in progress, or there is no rule.
    pub(crate) fn next_ms(&self) -> Option<i64> {
        if !self.enabled || self.pending.is_some() {
            return None;
        }
        let base = self.last_ms.or(self.configured_ms)?;
        if let Some(days) = self.after_days {
            return Some(day_of(base) + days * DAY_MS);
        }
        match aws_cron::parse(self.schedule.as_deref()?).ok()? {
            // A rate's windows start at midnight UTC.
            rate @ Schedule::Rate { .. } => rate.next_after(day_of(base), base),
            cron @ Schedule::Cron(_) => cron.next_after(0, base),
            // `aws_cron::parse` reads only rate() and cron(); at() is Scheduler's alone.
            Schedule::At { .. } => None,
        }
    }
}

/// Refuses the rules AWS documents as invalid: both `AutomaticallyAfterDays` and `ScheduleExpression`, an expression that is not
/// `rate()` or `cron()`, a rate in minutes or shorter than four hours. The words are (memory).
pub(crate) fn check_rules(r: &Rotation) -> Result<(), ApiError> {
    if r.after_days.is_some() && r.schedule.is_some() {
        // (memory)
        return Err(invalid_parameter(
            "You can't specify both AutomaticallyAfterDays and ScheduleExpression in RotationRules.",
        ));
    }
    let Some(expr) = r.schedule.as_deref() else {
        return Ok(());
    };
    match aws_cron::parse(expr) {
        Ok(Schedule::Rate { every_ms }) if expr.contains("minute") || every_ms < SHORTEST_MS => {
            // (memory)
            Err(invalid_parameter(
                "The rotation schedule can't be more frequent than every four hours.",
            ))
        }
        Ok(_) => Ok(()),
        // (memory)
        Err(_) => Err(invalid_parameter(format!(
            "The schedule expression {expr} isn't valid. Use a rate() or cron() expression."
        ))),
    }
}

impl Core {
    /// The soonest rotation window of the region, among the secrets that can rotate (rotation on, a function, not marked for
    /// deletion).
    pub(crate) fn next_due(&self) -> Option<i64> {
        self.rotations
            .iter()
            .filter(|(name, r)| r.function.is_some() && self.live(name))
            .filter_map(|(_, r)| r.next_ms())
            .min()
    }

    fn live(&self, name: &str) -> bool {
        self.secrets
            .get(name)
            .is_some_and(|s| s.deleted_ms.is_none())
    }

    /// The timer's tick: starts the rotation of every secret whose window has opened, as `RotateSecret` with no arguments would.
    pub(crate) fn tick(&mut self) -> Answer {
        let now = self.now_ms();
        let due: Vec<String> = self
            .rotations
            .iter()
            .filter(|(name, r)| {
                r.function.is_some() && self.live(name) && r.next_ms().is_some_and(|at| at <= now)
            })
            .map(|(name, _)| name.to_owned())
            .collect();
        for name in due {
            let Some(token) = aws_seal::uuid_v4() else {
                continue;
            };
            let Some(caller) = self.secrets.get(&name).map(|s| caller_of(&s.arn)) else {
                continue;
            };
            tracing::info!(secret = %name, "secretsmanager: the rotation window opened; rotating");
            self.begin(&name, token, caller);
        }
        let (reply, durable) = match self.flush() {
            Ok(ack) => (aws_wire::handler::empty(), ack),
            Err(e) => (
                Err(ApiError::internal(format!("could not save: {e}"))),
                None,
            ),
        };
        let runs = if reply.is_ok() {
            std::mem::take(&mut self.runs)
        } else {
            self.runs.clear();
            Vec::new()
        };
        Answer {
            reply,
            durable,
            runs,
            next_due: self.next_due(),
        }
    }

    /// Starts a full rotation of `name` under `token`: the pending version with no value, the rotation marked in progress, and the
    /// run handed back to be started once this is saved.
    pub(crate) fn begin(&mut self, name: &str, token: String, caller: Caller) {
        let now = self.now_ms();
        let Some(mut r) = self.rotations.get(name).cloned() else {
            return;
        };
        let (Some(function), Some(secret)) = (r.function.clone(), self.secrets.get_mut(name))
        else {
            return;
        };
        open_pending(secret, &token, now);
        let secret_arn = secret.arn.clone();
        r.pending = Some(token.clone());
        self.rotations.insert(name.to_owned(), r);
        self.runs.push(Run {
            function,
            secret_arn,
            token,
            caller,
            step: Step::All,
        });
    }

    /// A value a client put that became `AWSCURRENT` counts as a rotation for `AutomaticallyAfterDays` (the model: "Manually updating
    /// the secret value by calling `PutSecretValue` or `UpdateSecret` is considered a valid rotation").
    pub(crate) fn note_manual_rotation(&mut self, name: &str) {
        let now = self.now_ms();
        if let Some(r) = self.rotations.get(name)
            && r.enabled
            && r.pending.is_none()
            && r.after_days.is_some()
        {
            let mut r = r.clone();
            r.last_ms = Some(now);
            self.rotations.insert(name.to_owned(), r);
        }
    }
}

/// Who a scheduled rotation acts as: the secret's account and region, as Secrets Manager itself (no client is calling).
fn caller_of(arn: &str) -> Caller {
    let mut parts = arn.split(':');
    let region = parts.nth(3).unwrap_or_default().to_owned();
    let account_id = parts.next().unwrap_or_default().to_owned();
    Caller {
        access_key_id: "secretsmanager.amazonaws.com".to_owned(),
        account_id,
        region,
        endpoint: String::new(),
        service: Some("secretsmanager".to_owned()),
        session_token: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(after_days: Option<i64>, schedule: Option<&str>) -> Rotation {
        Rotation {
            enabled: true,
            function: Some("f".into()),
            configured_ms: Some(DAY_MS + 3_600_000 * 10),
            after_days,
            schedule: schedule.map(str::to_owned),
            ..Rotation::default()
        }
    }

    #[test]
    fn windows_open_at_midnight_for_days_and_rates_and_where_cron_says() {
        assert_eq!(rule(Some(30), None).next_ms(), Some(31 * DAY_MS));
        assert_eq!(
            rule(None, Some("rate(4 hours)")).next_ms(),
            Some(DAY_MS + 3_600_000 * 12),
            "the next four-hour window after 10:00 opens at 12:00"
        );
        assert_eq!(
            rule(None, Some("rate(10 days)")).next_ms(),
            Some(11 * DAY_MS)
        );
        assert_eq!(
            rule(None, Some("cron(0 16 * * ? *)")).next_ms(),
            Some(DAY_MS + 3_600_000 * 16)
        );
        let mut r = rule(Some(1), None);
        r.last_ms = Some(5 * DAY_MS + 7);
        assert_eq!(
            r.next_ms(),
            Some(6 * DAY_MS),
            "counted from the last rotation"
        );
        r.pending = Some("tok".into());
        assert_eq!(r.next_ms(), None, "not while one is in progress");
        r.pending = None;
        r.enabled = false;
        assert_eq!(r.next_ms(), None);
    }

    #[test]
    fn the_rules_aws_refuses_are_refused() {
        assert!(check_rules(&rule(Some(1), Some("rate(1 day)"))).is_err());
        assert!(check_rules(&rule(None, Some("rate(1 hour)"))).is_err());
        assert!(check_rules(&rule(None, Some("rate(300 minutes)"))).is_err());
        assert!(check_rules(&rule(None, Some("every day"))).is_err());
        assert!(check_rules(&rule(None, Some("rate(4 hours)"))).is_ok());
        assert!(check_rules(&rule(None, Some("cron(0 16 1,15 * ? *)"))).is_ok());
    }

    #[test]
    fn a_scheduled_rotation_acts_in_the_secrets_account_and_region() {
        let c = caller_of("arn:aws:secretsmanager:eu-west-1:111122223333:secret:db-abc123");
        assert_eq!(
            (c.region.as_str(), c.account_id.as_str()),
            ("eu-west-1", "111122223333")
        );
    }
}
