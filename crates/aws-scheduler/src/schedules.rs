//! Schedules: made, replaced whole by `UpdateSchedule` (a member left out goes back to its default, as AWS documents), read, deleted
//! and listed.

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::core::Core;
use crate::model::{Schedule, key, schedule_arn};
use crate::page::{page, secs, with_token};
use crate::req::{Req, conflict, invalid, no_group, no_schedule};

/// The retry policy a target has when it names none (documented: 24 hours and 185 attempts).
const DEFAULT_AGE_SECONDS: i64 = 86_400;
const DEFAULT_ATTEMPTS: i64 = 185;

impl Core {
    pub(crate) fn create_schedule(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (group, name) = (r.group(), r.s("Name"));
        if self.groups.get(&key(r.account(), &[group])).is_none() {
            return Err(no_group(group));
        }
        let k = key(r.account(), &[group, name]);
        if self.schedules.get(&k).is_some() {
            // (memory)
            return Err(conflict(format!("Schedule {name} already exists.")));
        }
        let now = self.now_ms();
        let mut s = self.read_schedule(r, now)?;
        s.created_ms = now;
        let arn = s.arn.clone();
        self.place(k, s);
        Ok(json!({"ScheduleArn": arn}))
    }

    pub(crate) fn update_schedule(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (group, name) = (r.group(), r.s("Name"));
        let k = key(r.account(), &[group, name]);
        let Some(created) = self.schedules.get(&k).map(|s| s.created_ms) else {
            return Err(no_schedule(name));
        };
        let now = self.now_ms();
        let mut s = self.read_schedule(r, now)?;
        s.created_ms = created;
        let arn = s.arn.clone();
        self.place(k, s);
        Ok(json!({"ScheduleArn": arn}))
    }

    /// Stores a schedule with its first fire planned. A rate with no `StartDate` fires at once (documented: "your schedule starts
    /// invoking the target immediately").
    fn place(&mut self, k: String, mut s: Schedule) {
        s.next_ms = crate::plan::next(&s, s.anchor_ms - 1);
        self.schedules.insert(k, s);
    }

    pub(crate) fn get_schedule(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (group, name) = (r.group(), r.s("Name"));
        let s = self
            .schedules
            .get(&key(r.account(), &[group, name]))
            .ok_or_else(|| no_schedule(name))?;
        Ok(describe(s))
    }

    pub(crate) fn delete_schedule(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (group, name) = (r.group(), r.s("Name"));
        self.schedules
            .remove(&key(r.account(), &[group, name]))
            .ok_or_else(|| no_schedule(name))?;
        Ok(Value::Null)
    }

    pub(crate) fn list_schedules(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let group = r.opt("GroupName");
        if let Some(g) = group
            && self.groups.get(&key(r.account(), &[g])).is_none()
        {
            return Err(no_group(g));
        }
        let prefix = match group {
            Some(g) => key(r.account(), &[g]) + "/",
            None => key(r.account(), &[]) + "/",
        };
        let (name_prefix, state) = (r.s("NamePrefix"), r.opt("State"));
        let items = self
            .schedules
            .with_prefix(&prefix)
            .filter(|(_, s)| s.name.starts_with(name_prefix))
            .filter(|(_, s)| state.is_none_or(|st| st == state_of(s)))
            .map(|(k, s)| (k.to_owned(), summary(s)));
        let (list, token) = page(items, r)?;
        Ok(with_token(json!({"Schedules": list}), token))
    }

    /// Reads and checks the members `CreateSchedule` and `UpdateSchedule` share.
    fn read_schedule(&self, r: &Req<'_>, now: i64) -> Result<Schedule, ApiError> {
        let (group, name) = (r.group(), r.s("Name"));
        let expression = r.s("ScheduleExpression").to_owned();
        let parsed = aws_cron::parse_scheduler(&expression)
            // (memory)
            .map_err(|_| invalid(format!("Invalid Schedule Expression {expression}.")))?;
        let timezone = r.opt("ScheduleExpressionTimezone").map(str::to_owned);
        if let Some(tz) = &timezone
            && !aws_cron::Zone::known(tz)
        {
            // (memory)
            return Err(invalid(format!("Invalid time zone {tz}.")));
        }
        let (start_ms, end_ms) = (r.time_ms("StartDate"), r.time_ms("EndDate"));
        if !parsed.is_once() {
            crate::check::dates(start_ms, end_ms, now)?;
        }
        let window =
            r.v.get("FlexibleTimeWindow")
                .cloned()
                .unwrap_or(Value::Null);
        crate::check::window(&window)?;
        let target = r.v.get("Target").cloned().unwrap_or(Value::Null);
        crate::targets::check(&target)?;
        Ok(Schedule {
            account: r.account().to_owned(),
            group: group.to_owned(),
            name: name.to_owned(),
            arn: schedule_arn(r.region(), r.account(), group, name),
            expression,
            timezone,
            start_ms,
            end_ms,
            description: r.opt("Description").map(str::to_owned),
            enabled: r.opt("State") != Some("DISABLED"),
            kms_key: r.opt("KmsKeyArn").map(str::to_owned),
            target: target.to_string(),
            window: window.to_string(),
            delete_after: r.opt("ActionAfterCompletion") == Some("DELETE"),
            created_ms: now,
            modified_ms: now,
            anchor_ms: now,
            next_ms: None,
        })
    }
}

fn state_of(s: &Schedule) -> &'static str {
    if s.enabled { "ENABLED" } else { "DISABLED" }
}

/// The target as `GetSchedule` shows it: as sent, with the retry policy's defaults filled in.
pub(crate) fn target_of(s: &Schedule) -> Value {
    let mut t: Value = serde_json::from_str(&s.target).unwrap_or_else(|_| json!({}));
    if let Some(m) = t.as_object_mut() {
        let policy = m
            .entry("RetryPolicy")
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(p) = policy.as_object_mut() {
            p.entry("MaximumEventAgeInSeconds")
                .or_insert(json!(DEFAULT_AGE_SECONDS));
            p.entry("MaximumRetryAttempts")
                .or_insert(json!(DEFAULT_ATTEMPTS));
        }
    }
    t
}

/// A schedule as `GetSchedule` answers it.
fn describe(s: &Schedule) -> Value {
    let mut out = json!({
        "Arn": s.arn,
        "GroupName": s.group,
        "Name": s.name,
        "ScheduleExpression": s.expression,
        // (memory) AWS answers UTC for a schedule made without a zone.
        "ScheduleExpressionTimezone": s.timezone.as_deref().unwrap_or("UTC"),
        "State": state_of(s),
        "CreationDate": secs(s.created_ms),
        "LastModificationDate": secs(s.modified_ms),
        "Target": target_of(s),
        "FlexibleTimeWindow": serde_json::from_str::<Value>(&s.window).unwrap_or(Value::Null),
        "ActionAfterCompletion": if s.delete_after { "DELETE" } else { "NONE" },
    });
    if let Some(m) = out.as_object_mut() {
        let mut put = |k: &str, v: Option<Value>| {
            if let Some(v) = v {
                m.insert(k.to_owned(), v);
            }
        };
        put("StartDate", s.start_ms.map(secs));
        put("EndDate", s.end_ms.map(secs));
        put("Description", s.description.clone().map(Value::String));
        put("KmsKeyArn", s.kms_key.clone().map(Value::String));
    }
    out
}

/// A schedule as `ListSchedules` answers it.
fn summary(s: &Schedule) -> Value {
    let arn = serde_json::from_str::<Value>(&s.target)
        .ok()
        .and_then(|t| t.get("Arn").cloned())
        .unwrap_or(Value::Null);
    json!({
        "Arn": s.arn,
        "Name": s.name,
        "GroupName": s.group,
        "State": state_of(s),
        "CreationDate": secs(s.created_ms),
        "LastModificationDate": secs(s.modified_ms),
        "Target": {"Arn": arn},
    })
}
