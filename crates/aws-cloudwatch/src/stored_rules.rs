//! Contributor Insights rules and alarm mute rules: stored, returned, tagged, and not active locally (see `stored.rs`).

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::req::{Put, Req, secs};
use crate::stored::{MUTE, RULE, arn, not_found, page, strip};

/// What a rule's `Schema` says for a rule a caller made (the model's documentation of `InsightRule`).
const LOG_RULE_SCHEMA: &str = r#"{"Name": "CloudWatchLogRule", "Version": 1}"#;

impl Core {
    pub(crate) fn put_insight_rule(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("RuleName").to_owned();
        let definition = r.s("RuleDefinition");
        if !serde_json::from_str::<Value>(definition).is_ok_and(|v| v.is_object()) {
            // (memory)
            return Err(ApiError::modeled(
                "InvalidParameterValueException",
                "The rule definition is not valid JSON.",
            ));
        }
        let state = r.opt("RuleState").unwrap_or("ENABLED");
        let body = json!({
            "State": state,
            "Definition": definition,
            "ApplyOnTransformedLogs": r.flag("ApplyOnTransformedLogs").unwrap_or(false),
        });
        self.keep(RULE, &name, body, r);
        Ok(json!({}))
    }

    pub(crate) fn describe_insight_rules(&self, r: &Req<'_>) -> Value {
        let rules: Vec<Value> = self
            .all(RULE)
            .into_iter()
            .map(|(name, v, _)| {
                json!({
                    "Name": name,
                    "State": v.get("State"),
                    "Schema": LOG_RULE_SCHEMA,
                    "Definition": v.get("Definition"),
                    "ManagedRule": false,
                    "ApplyOnTransformedLogs": v.get("ApplyOnTransformedLogs"),
                })
            })
            .collect();
        page(rules, r, "MaxResults", "InsightRules")
    }

    /// `DeleteInsightRules`: the names not found are failures (memory).
    pub(crate) fn delete_insight_rules(&mut self, r: &Req<'_>) -> Value {
        let names = r.strings("RuleNames");
        let missing: Vec<String> = names
            .iter()
            .filter(|n| self.one(RULE, n).is_none())
            .cloned()
            .collect();
        for n in &names {
            self.delete_stored(RULE, n);
        }
        json!({"Failures": crate::stored::failures(&missing)})
    }

    /// An empty report in AWS's shape: the keys the rule's `Contribution` names, and no contributor (nothing is counted locally).
    pub(crate) fn get_insight_rule_report(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("RuleName");
        let (v, _) = self
            .one(RULE, name)
            .ok_or_else(|| not_found("rule", name))?;
        let definition: Value = v
            .get("Definition")
            .and_then(Value::as_str)
            .and_then(|d| serde_json::from_str(d).ok())
            .unwrap_or(Value::Null);
        let keys = definition
            .pointer("/Contribution/Keys")
            .cloned()
            .unwrap_or_else(|| json!([]));
        let statistic = definition
            .get("AggregateOn")
            .and_then(Value::as_str)
            .unwrap_or("Count");
        Ok(json!({
            "KeyLabels": keys,
            "AggregationStatistic": statistic,
            "AggregateValue": 0.0,
            "ApproximateUniqueCount": 0,
            "Contributors": [],
            "MetricDatapoints": [],
        }))
    }

    pub(crate) fn put_alarm_mute_rule(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("Name").to_owned();
        let expression =
            r.v.pointer("/Rule/Schedule/Expression")
                .and_then(Value::as_str)
                .unwrap_or_default();
        if !(expression.starts_with("cron(") || expression.starts_with("at(")) {
            // (memory)
            return Err(ApiError::modeled(
                "InvalidParameterValueException",
                "The schedule expression must be a cron() or at() expression.",
            ));
        }
        let mut body = json!({});
        for k in ["Description", "Rule", "MuteTargets"] {
            if let Some(v) = r.v.get(k) {
                body.put(k, v.clone());
            }
        }
        for k in ["StartDate", "ExpireDate"] {
            if let Some(ms) = r.time(k) {
                body.put(&format!("_{k}"), json!(ms));
            }
        }
        self.keep(MUTE, &name, body, r);
        Ok(json!({}))
    }

    fn mute_json(&self, r: &Req<'_>, name: &str, v: &Value, updated_ms: i64) -> Value {
        let now = self.now_ms();
        let at = |k: &str| v.get(format!("_{k}")).and_then(Value::as_i64);
        let status = match (at("StartDate"), at("ExpireDate")) {
            (_, Some(end)) if end <= now => "EXPIRED",
            (Some(start), _) if start > now => "SCHEDULED",
            _ => "ACTIVE",
        };
        let one_time = v
            .pointer("/Rule/Schedule/Expression")
            .and_then(Value::as_str)
            .is_some_and(|e| e.starts_with("at("));
        let mut out = v.clone();
        for k in ["StartDate", "ExpireDate"] {
            if let Some(ms) = at(k) {
                out.put(k, secs(ms));
            }
        }
        strip(&mut out);
        out.put("Name", json!(name));
        out.put("AlarmMuteRuleArn", json!(arn(r, MUTE, name)));
        out.put("Status", json!(status));
        out.put(
            "MuteType",
            json!(if one_time { "ONE_TIME" } else { "RECURRING" }),
        );
        out.put("LastUpdatedTimestamp", secs(updated_ms));
        out
    }

    pub(crate) fn get_alarm_mute_rule(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("AlarmMuteRuleName");
        let (v, at) = self
            .one(MUTE, name)
            .ok_or_else(|| not_found("alarm mute rule", name))?;
        Ok(self.mute_json(r, name, &v, at))
    }

    pub(crate) fn list_alarm_mute_rules(&self, r: &Req<'_>) -> Value {
        let alarm = r.opt("AlarmName");
        let statuses = r.strings("Statuses");
        let rules: Vec<Value> = self
            .all(MUTE)
            .iter()
            .map(|(name, v, at)| self.mute_json(r, name, v, *at))
            .filter(|m| {
                alarm.is_none_or(|a| {
                    m.pointer("/MuteTargets/AlarmNames")
                        .and_then(Value::as_array)
                        .is_some_and(|l| l.iter().any(|x| x.as_str() == Some(a)))
                })
            })
            .filter(|m| {
                statuses.is_empty() || statuses.iter().any(|s| m["Status"].as_str() == Some(s))
            })
            .map(|m| {
                let keep = [
                    "AlarmMuteRuleArn",
                    "ExpireDate",
                    "Status",
                    "MuteType",
                    "LastUpdatedTimestamp",
                ];
                Value::Object(
                    keep.iter()
                        .filter_map(|k| Some(((*k).to_owned(), m.get(*k)?.clone())))
                        .collect(),
                )
            })
            .collect();
        page(rules, r, "MaxRecords", "AlarmMuteRuleSummaries")
    }
}
