//! Rules: an event pattern or a schedule, a state, and the targets (`targets.rs`).

use aws_wire::ApiError;
use serde_json::Value;

use crate::core::Core;
use crate::model::{Rule, key};
use crate::names::{DEFAULT_BUS, rule_arn};
use crate::page::{page, with_token};
use crate::pattern::{self, Pattern};
use crate::req::{Req, invalid, invalid_pattern, no_rule};
use crate::util::Out;

/// The members of an event that `TestEventPattern` insists on (documented).
const REQUIRED_EVENT_FIELDS: [&str; 7] = [
    "id",
    "account",
    "source",
    "time",
    "region",
    "resources",
    "detail-type",
];

impl Core {
    /// The rule `name` on the bus a request names.
    pub(crate) fn rule(
        &self,
        r: &Req<'_>,
        bus_member: &str,
        name: &str,
    ) -> Result<&Rule, ApiError> {
        let bus = self.bus(r.account(), r.opt(bus_member))?.name.clone();
        self.rules
            .get(&key(r.account(), &[&bus, name]))
            .ok_or_else(|| no_rule(&bus, name))
    }

    /// The rule's pattern, read once and kept until the rule changes. `None` for a rule that has none.
    pub(crate) fn pattern_of(&mut self, rule_key: &str) -> Option<&Pattern> {
        if !self.compiled.contains_key(rule_key) {
            let text = self.rules.get(rule_key)?.pattern.clone()?;
            // WHY: a pattern in the store was checked when it was put; one that no longer reads is skipped, not matched.
            let pattern = pattern::parse(&text).ok()?;
            self.compiled.insert(rule_key.to_owned(), pattern);
        }
        self.compiled.get(rule_key)
    }

    pub(crate) fn put_rule(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let bus = self.bus(r.account(), r.opt("EventBusName"))?.name.clone();
        let (pattern, schedule) = (r.opt("EventPattern"), r.opt("ScheduleExpression"));
        if pattern.is_none() && schedule.is_none() {
            return Err(invalid(
                "Parameter(s) EventPattern or ScheduleExpression must be specified.",
            ));
        }
        if let Some(text) = pattern {
            pattern::parse(text).map_err(|e| invalid_pattern(e.message(text)))?;
        }
        if let Some(text) = schedule {
            if bus != DEFAULT_BUS {
                return Err(invalid(
                    "ScheduleExpression is supported only on the default event bus.",
                ));
            }
            // AWS gives this one sentence for every expression it refuses (recorded 2026-11-09).
            aws_cron::parse(text)
                .map_err(|_| invalid("Parameter ScheduleExpression is not valid."))?;
        }
        let name = r.s("Name");
        let k = key(r.account(), &[&bus, name]);
        let arn = rule_arn(r.region(), r.account(), &bus, name);
        let now = self.now_ms();
        let enabled = !matches!(r.opt("State"), Some("DISABLED"));
        let old = self.rules.get(&k);
        // (memory) PutRule replaces the rule's settings, as a PUT does; its targets and tags stay.
        let anchor_ms = match old {
            Some(o) if o.schedule.as_deref() == schedule => o.anchor_ms,
            _ => now,
        };
        let (targets, mut tags) =
            old.map_or_else(Default::default, |o| (o.targets.clone(), o.tags.clone()));
        tags.extend(r.tags());
        let rule = Rule {
            account: r.account().to_owned(),
            bus,
            name: name.to_owned(),
            arn: arn.clone(),
            pattern: pattern.map(str::to_owned),
            schedule: schedule.map(str::to_owned),
            enabled,
            description: r.opt("Description").map(str::to_owned),
            role_arn: r.opt("RoleArn").map(str::to_owned),
            managed_by: None,
            tags,
            targets,
            anchor_ms,
        };
        self.forget(&k);
        self.rules.insert(k, rule);
        Ok(Out::new().with("RuleArn", arn).done())
    }

    /// Drops what was worked out about a rule that is changing.
    pub(crate) fn forget(&mut self, rule_key: &str) {
        self.compiled.remove(rule_key);
        self.due.remove(rule_key);
    }

    pub(crate) fn delete_rule(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let Ok(bus) = self
            .bus(r.account(), r.opt("EventBusName"))
            .map(|b| b.name.clone())
        else {
            // (memory) deleting from a bus that is gone is not an error, as deleting a rule that is gone is not.
            return Ok(Value::Null);
        };
        let k = key(r.account(), &[&bus, r.s("Name")]);
        if let Some(rule) = self.rules.get(&k) {
            if !rule.targets.is_empty() {
                return Err(invalid("Rule can't be deleted since it has targets."));
            }
            self.forget(&k);
            self.rules.remove(&k);
        }
        Ok(Value::Null)
    }

    pub(crate) fn describe_rule(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let rule = self.rule(r, "EventBusName", r.s("Name"))?;
        Ok(Out::new()
            .with("Name", rule.name.clone())
            .with("Arn", rule.arn.clone())
            .maybe("EventPattern", rule.pattern.clone())
            .maybe("ScheduleExpression", rule.schedule.clone())
            .with("State", state(rule))
            .maybe("Description", rule.description.clone())
            .maybe("RoleArn", rule.role_arn.clone())
            .maybe("ManagedBy", rule.managed_by.clone())
            .with("EventBusName", rule.bus.clone())
            .with("CreatedBy", rule.account.clone())
            .done())
    }

    pub(crate) fn list_rules(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let bus = self.bus(r.account(), r.opt("EventBusName"))?.name.clone();
        let prefix = r.opt("NamePrefix").unwrap_or_default();
        let mine = key(r.account(), &[&bus, ""]);
        let items = self
            .rules
            .with_prefix(&mine)
            .filter(|(_, rule)| rule.name.starts_with(prefix))
            .map(|(k, rule)| (k.to_owned(), rule));
        let (rules, token) = page(items, r, "Limit")?;
        let list: Vec<Value> = rules
            .into_iter()
            .map(|rule| {
                Out::new()
                    .with("Name", rule.name.clone())
                    .with("Arn", rule.arn.clone())
                    .maybe("EventPattern", rule.pattern.clone())
                    .with("State", state(rule))
                    .maybe("Description", rule.description.clone())
                    .maybe("ScheduleExpression", rule.schedule.clone())
                    .maybe("RoleArn", rule.role_arn.clone())
                    .maybe("ManagedBy", rule.managed_by.clone())
                    .with("EventBusName", rule.bus.clone())
                    .done()
            })
            .collect();
        Ok(with_token(Out::new().with("Rules", list).done(), token))
    }

    pub(crate) fn set_enabled(&mut self, r: &Req<'_>, enabled: bool) -> Result<Value, ApiError> {
        let (bus, name) = {
            let rule = self.rule(r, "EventBusName", r.s("Name"))?;
            (rule.bus.clone(), rule.name.clone())
        };
        let k = key(r.account(), &[&bus, &name]);
        self.forget(&k);
        if let Some(rule) = self.rules.get_mut(&k) {
            rule.enabled = enabled;
        }
        Ok(Value::Null)
    }

    pub(crate) fn test_event_pattern(r: &Req<'_>) -> Result<Value, ApiError> {
        let text = r.s("EventPattern");
        let pattern = pattern::parse(text).map_err(|e| invalid_pattern(e.message(text)))?;
        let event: Value = serde_json::from_str(r.s("Event"))
            .ok()
            .filter(|e: &Value| REQUIRED_EVENT_FIELDS.iter().all(|f| e.get(f).is_some()))
            // (memory) the wording.
            .ok_or_else(|| invalid("Parameter Event is not valid."))?;
        Ok(Out::new()
            .with("Result", pattern::matches(&pattern, &event))
            .done())
    }
}

fn state(rule: &Rule) -> &'static str {
    if rule.enabled { "ENABLED" } else { "DISABLED" }
}
