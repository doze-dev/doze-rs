//! Alarm actions. An action that names an SNS topic or a Lambda function is carried out through the peer bus when that service is
//! running; any other (or one whose service is not there) is not delivered, and the alarm's history says so: "stored, not
//! delivered locally" is the flag a client can see.
//!
//! The SNS message is the document AWS sends for an alarm. Its field names and layout are from AWS's documentation and memory; no
//! recording of it exists.

use aws_wire::service::Caller;
use serde_json::{Value, json};

use crate::alarms::{alarm_arn, iso};
use crate::core::{Core, Outgoing};
use crate::model::{Alarm, Kind};

impl Core {
    /// Carries out the queued actions, and notes in each alarm's history how it went.
    pub(crate) async fn deliver(&mut self, caller: &Caller) {
        let queued = std::mem::take(&mut self.outbox);
        for o in queued {
            let Some(alarm) = self.alarms.get(&o.alarm).cloned() else {
                continue;
            };
            let message = notification(&o, &alarm, caller);
            let outcome = self.send(&o, &message, caller).await;
            let (summary, state) = match &outcome {
                Ok(()) => (
                    format!("Successfully executed action {}", o.action),
                    "Succeeded",
                ),
                Err(why) => (
                    format!("Failed to execute action {}: {why}", o.action),
                    "Failed",
                ),
            };
            let data = json!({"actionState": state, "notificationResource": o.action, "stateUpdateTimestamp": iso(o.at_ms)}).to_string();
            let alarm_type = crate::alarms::type_of(alarm.kind);
            self.record(
                &o.alarm,
                self.now_ms(),
                ("Action", &summary, &data, alarm_type),
            );
        }
    }

    async fn send(&self, o: &Outgoing, message: &Value, caller: &Caller) -> Result<(), String> {
        let service = o.action.split(':').nth(2).unwrap_or_default();
        let (target, operation, body) = match service {
            "sns" => (
                "sns",
                "Publish",
                json!({
                    "TopicArn": o.action,
                    "Message": message.to_string(),
                    "Subject": format!("{}: \"{}\" in {}", o.new, o.alarm, region_name(&caller.region)),
                }),
            ),
            "lambda" => (
                "lambda",
                "Invoke",
                json!({
                    "FunctionName": o.action,
                    "InvocationType": "Event",
                    "Payload": aws_core::base64::encode(message.to_string().as_bytes()),
                }),
            ),
            other => {
                return Err(format!(
                    "actions on {} are stored and not delivered locally",
                    if other.is_empty() {
                        "this target"
                    } else {
                        other
                    }
                ));
            }
        };
        let Some(peers) = self.peers.clone() else {
            return Err(format!(
                "{target} is not running, so the action is stored and not delivered locally"
            ));
        };
        peers
            .call("monitoring", caller, target, operation, &body)
            .await
            .map(drop)
            .map_err(|e| {
                if e.message.starts_with("no service") {
                    format!(
                        "{target} is not running, so the action is stored and not delivered locally"
                    )
                } else {
                    e.message
                }
            })
    }
}

/// The message AWS publishes for an alarm that changed state.
fn notification(o: &Outgoing, a: &Alarm, caller: &Caller) -> Value {
    let (region, account) = (&caller.region, &caller.account_id);
    let mut m = json!({
        "AlarmName": o.alarm,
        "AlarmDescription": serde_json::from_str::<Value>(&a.request).ok()
            .and_then(|v| v.get("AlarmDescription").cloned()).unwrap_or(Value::Null),
        "AWSAccountId": account,
        "AlarmConfigurationUpdatedTimestamp": iso(a.config_updated_ms),
        "NewStateValue": o.new,
        "NewStateReason": a.reason,
        "StateChangeTime": iso(o.at_ms),
        "Region": region_name(region),
        "AlarmArn": alarm_arn(region, account, &o.alarm),
        "OldStateValue": o.old,
        "OKActions": a.ok_actions,
        "AlarmActions": a.alarm_actions,
        "InsufficientDataActions": a.insufficient_actions,
    });
    if a.kind == Kind::Metric {
        let s = &a.spec;
        if let Value::Object(map) = &mut m {
            map.insert(
                "Trigger".to_owned(),
                json!({
                    "MetricName": s.metric,
                    "Namespace": s.namespace,
                    "StatisticType": "Statistic",
                    "Statistic": s.stat.to_ascii_uppercase(),
                    "Unit": s.unit,
                    "Dimensions": s.dims.iter().map(|(n, v)| json!({"value": v, "name": n})).collect::<Vec<_>>(),
                    "Period": s.period_s,
                    "EvaluationPeriods": s.evaluation_periods,
                    "DatapointsToAlarm": s.datapoints_to_alarm,
                    "ComparisonOperator": s.comparison,
                    "Threshold": s.threshold,
                    "TreatMissingData": s.treat_missing,
                    "EvaluateLowSampleCountPercentile": "",
                }),
            );
        }
    }
    m
}

/// The name SNS puts in an alarm message for a region.
fn region_name(region: &str) -> String {
    match region {
        "us-east-1" => "US East (N. Virginia)",
        "us-east-2" => "US East (Ohio)",
        "us-west-1" => "US West (N. California)",
        "us-west-2" => "US West (Oregon)",
        "eu-west-1" => "EU (Ireland)",
        "eu-west-2" => "EU (London)",
        "eu-central-1" => "EU (Frankfurt)",
        "ap-southeast-1" => "Asia Pacific (Singapore)",
        "ap-southeast-2" => "Asia Pacific (Sydney)",
        "ap-northeast-1" => "Asia Pacific (Tokyo)",
        other => other,
    }
    .to_owned()
}
