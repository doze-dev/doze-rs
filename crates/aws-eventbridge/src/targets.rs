//! Targets of a rule: put, remove, list. A target is kept as the JSON it was sent as, so every parameter block comes back from
//! `ListTargetsByRule` as it went in, whether or not anything here acts on it.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::input;
use crate::model::{StoredTarget, key};
use crate::page::{page, with_token};
use crate::req::{Req, limit, no_rule};
use crate::util::Out;

/// The most targets a rule has (AWS's default quota).
const MAX_TARGETS: usize = 5;

impl Core {
    pub(crate) fn put_targets(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (bus, name) = {
            let rule = self.rule(r, "EventBusName", r.s("Rule"))?;
            (rule.bus.clone(), rule.name.clone())
        };
        let k = key(r.account(), &[&bus, &name]);
        let mut failed = Vec::new();
        let mut accepted = Vec::new();
        for t in r.list("Targets") {
            let id = t.get("Id").and_then(Value::as_str).unwrap_or_default();
            match input::check_target(t) {
                Ok(()) => accepted.push(stored(id, t)),
                Err(why) => failed.push(json!({
                    "TargetId": id, "ErrorCode": "ValidationException", "ErrorMessage": why,
                })),
            }
        }
        let rule = self.rules.get_mut(&k).ok_or_else(|| no_rule(&bus, &name))?;
        let mut targets = rule.targets.clone();
        for t in accepted {
            targets.retain(|old| old.id != t.id);
            targets.push(t);
        }
        if targets.len() > MAX_TARGETS {
            return Err(limit(
                "The requested resource exceeds the maximum number allowed.",
            ));
        }
        rule.targets = targets;
        self.due.remove(&k);
        Ok(Out::new()
            .with("FailedEntryCount", failed.len())
            .with("FailedEntries", failed)
            .done())
    }

    pub(crate) fn remove_targets(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (bus, name) = {
            let rule = self.rule(r, "EventBusName", r.s("Rule"))?;
            (rule.bus.clone(), rule.name.clone())
        };
        let ids: Vec<&str> = r.list("Ids").iter().filter_map(Value::as_str).collect();
        let k = key(r.account(), &[&bus, &name]);
        if let Some(rule) = self.rules.get_mut(&k) {
            // An id that is not there is not a failure (recorded 2026-11-09).
            rule.targets.retain(|t| !ids.contains(&t.id.as_str()));
        }
        self.due.remove(&k);
        Ok(Out::new()
            .with("FailedEntryCount", 0)
            .with("FailedEntries", Vec::<Value>::new())
            .done())
    }

    pub(crate) fn list_targets(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let rule = self.rule(r, "EventBusName", r.s("Rule"))?;
        let items = rule.targets.iter().map(|t| {
            (
                t.id.clone(),
                serde_json::from_str::<Value>(&t.json).unwrap_or(Value::Null),
            )
        });
        let (targets, token) = page(items, r, "Limit")?;
        Ok(with_token(
            Out::new().with("Targets", targets).done(),
            token,
        ))
    }

    pub(crate) fn list_rule_names_by_target(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let bus = self.bus(r.account(), r.opt("EventBusName"))?.name.clone();
        let arn = r.s("TargetArn");
        let mine = key(r.account(), &[&bus, ""]);
        let items = self
            .rules
            .with_prefix(&mine)
            .filter(|(_, rule)| rule.targets.iter().any(|t| t.arn == arn))
            .map(|(k, rule)| (k.to_owned(), rule.name.clone()));
        let (names, token) = page(items, r, "Limit")?;
        Ok(with_token(
            Out::new().with("RuleNames", names).done(),
            token,
        ))
    }
}

fn stored(id: &str, target: &Value) -> StoredTarget {
    StoredTarget {
        id: id.to_owned(),
        arn: target
            .get("Arn")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        json: target.to_string(),
    }
}
