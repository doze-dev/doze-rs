//! `AWS::CloudWatch::Alarm` (single-metric and metric-math), `CompositeAlarm` and `Dashboard`.

use serde_json::{Value, json};

use super::{Done, Made, Rc, document, gone, tag_list, text};
use crate::j::J;
use crate::model::Res;

const SVC: &str = "monitoring";

const ALARM: &[&str] = &[
    "AlarmDescription",
    "ActionsEnabled",
    "AlarmActions",
    "OKActions",
    "InsufficientDataActions",
    "MetricName",
    "Namespace",
    "Statistic",
    "ExtendedStatistic",
    "Dimensions",
    "Period",
    "Unit",
    "EvaluationPeriods",
    "DatapointsToAlarm",
    "Threshold",
    "ComparisonOperator",
    "TreatMissingData",
    "EvaluateLowSampleCountPercentile",
    "Metrics",
    "ThresholdMetricId",
];

const COMPOSITE: &[&str] = &[
    "AlarmDescription",
    "AlarmRule",
    "ActionsEnabled",
    "AlarmActions",
    "OKActions",
    "InsufficientDataActions",
    "ActionsSuppressor",
    "ActionsSuppressorWaitPeriod",
    "ActionsSuppressorExtensionPeriod",
];

fn pick(p: &Value, keys: &[&str]) -> Value {
    let mut out = json!({});
    for k in keys {
        if let Some(v) = p.get(*k).filter(|v| !v.is_null()) {
            out.put(k, v.clone());
        }
    }
    out
}

fn alarm_arn(rc: &Rc, name: &str) -> String {
    format!(
        "arn:aws:cloudwatch:{}:{}:alarm:{name}",
        rc.caller.region, rc.caller.account_id
    )
}

async fn put(rc: &Rc, name: &str, p: &Value, first: bool) -> Result<(), String> {
    let (op, keys) = if rc.ty == "AWS::CloudWatch::CompositeAlarm" {
        ("PutCompositeAlarm", COMPOSITE)
    } else {
        ("PutMetricAlarm", ALARM)
    };
    let mut body = pick(p, keys);
    body.put("AlarmName", name.into());
    let tags: Vec<Value> = tag_list(rc, p)
        .into_iter()
        .map(|(k, v)| json!({"Key": k, "Value": v}))
        .collect();
    if first && !tags.is_empty() {
        body.put("Tags", Value::Array(tags));
    }
    rc.call(SVC, op, body)
        .await
        .map(|_| ())
        .map_err(|e| rc.fail(SVC, &e))
}

pub(super) async fn create(rc: &Rc, p: &Value) -> Done {
    if rc.ty == "AWS::CloudWatch::Dashboard" {
        let name =
            text(p, "DashboardName").unwrap_or_else(|| super::generated_name(rc, 255, false));
        let body = json!({"DashboardName": name, "DashboardBody": document(p, "DashboardBody")});
        rc.call(SVC, "PutDashboard", body)
            .await
            .map_err(|e| rc.fail(SVC, &e))?;
        return Ok(Made::new(name));
    }
    let name = text(p, "AlarmName").unwrap_or_else(|| super::generated_name(rc, 255, false));
    let found = rc
        .call(SVC, "DescribeAlarms", json!({"AlarmNames": [name]}))
        .await;
    let taken = found.is_ok_and(|f| {
        f.at("MetricAlarms")
            .as_array()
            .is_some_and(|a| !a.is_empty())
            || f.at("CompositeAlarms")
                .as_array()
                .is_some_and(|a| !a.is_empty())
    });
    if taken {
        return Err(rc.taken(&name));
    }
    put(rc, &name, p, true).await?;
    Ok(Made::new(&name).with("Arn", alarm_arn(rc, &name)))
}

pub(super) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    let name = old.physical.clone().unwrap_or_default();
    if rc.ty == "AWS::CloudWatch::Dashboard" {
        let body = json!({"DashboardName": name, "DashboardBody": document(p, "DashboardBody")});
        rc.call(SVC, "PutDashboard", body)
            .await
            .map_err(|e| rc.fail(SVC, &e))?;
        return Ok(Made::new(name));
    }
    put(rc, &name, p, false).await?;
    let (then, now) = (tag_list(rc, &old.props_value()), tag_list(rc, p));
    if then != now {
        let arn = alarm_arn(rc, &name);
        let removed: Vec<&String> = then
            .iter()
            .map(|(k, _)| k)
            .filter(|k| !now.iter().any(|(n, _)| n == *k))
            .collect();
        if !removed.is_empty() {
            rc.call(
                SVC,
                "UntagResource",
                json!({"ResourceARN": arn, "TagKeys": removed}),
            )
            .await
            .map_err(|e| rc.fail(SVC, &e))?;
        }
        let tags: Vec<Value> = now
            .iter()
            .map(|(k, v)| json!({"Key": k, "Value": v}))
            .collect();
        if !tags.is_empty() {
            rc.call(
                SVC,
                "TagResource",
                json!({"ResourceARN": arn, "Tags": tags}),
            )
            .await
            .map_err(|e| rc.fail(SVC, &e))?;
        }
    }
    Ok(Made::new(&name).with("Arn", alarm_arn(rc, &name)))
}

pub(super) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    let name = res.physical.clone().unwrap_or_default();
    let (op, body) = if rc.ty == "AWS::CloudWatch::Dashboard" {
        ("DeleteDashboards", json!({"DashboardNames": [name]}))
    } else {
        ("DeleteAlarms", json!({"AlarmNames": [name]}))
    };
    match rc.call(SVC, op, body).await {
        Ok(_) => Ok(()),
        Err(e) if gone(&e) => Ok(()),
        Err(e) => Err(rc.fail(SVC, &e)),
    }
}
