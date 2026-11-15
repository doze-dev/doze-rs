//! `AWS::Logs::LogGroup`, `LogStream`, `MetricFilter`, `SubscriptionFilter`, `ResourcePolicy` and `QueryDefinition`. CloudWatch
//! Logs spells its members in lower camel case (`logGroupName`), so properties are renamed on the way.

use serde_json::{Map, Value, json};

use super::{Done, Made, Rc, document, gone, tag_list, text};
use crate::j::J;
use crate::model::Res;

const SVC: &str = "logs";

/// `MetricName` to `metricName`, all the way down.
pub(super) fn camel(v: &Value) -> Value {
    match v {
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, x)| {
                    let mut c = k.clone();
                    if let Some(f) = c.get_mut(..1) {
                        f.make_ascii_lowercase();
                    }
                    (c, camel(x))
                })
                .collect::<Map<String, Value>>(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(camel).collect()),
        other => other.clone(),
    }
}

fn group_arn(rc: &Rc, name: &str) -> String {
    format!(
        "arn:aws:logs:{}:{}:log-group:{name}:*",
        rc.caller.region, rc.caller.account_id
    )
}

pub(super) async fn create(rc: &Rc, p: &Value) -> Done {
    match rc.ty.as_str() {
        "AWS::Logs::LogGroup" => group(rc, p).await,
        "AWS::Logs::LogStream" => {
            let name =
                text(p, "LogStreamName").unwrap_or_else(|| super::generated_name(rc, 512, false));
            rc.call(
                SVC,
                "CreateLogStream",
                json!({"logGroupName": text(p, "LogGroupName"), "logStreamName": name}),
            )
            .await
            .map_err(|e| rc.fail(SVC, &e))?;
            Ok(Made::new(name))
        }
        "AWS::Logs::MetricFilter" => {
            let name =
                text(p, "FilterName").unwrap_or_else(|| super::generated_name(rc, 512, false));
            put_metric_filter(rc, &name, p).await?;
            Ok(Made::new(name))
        }
        "AWS::Logs::SubscriptionFilter" => {
            let name =
                text(p, "FilterName").unwrap_or_else(|| super::generated_name(rc, 512, false));
            put_subscription(rc, &name, p).await?;
            Ok(Made::new(name))
        }
        "AWS::Logs::ResourcePolicy" => {
            let name = text(p, "PolicyName").unwrap_or_default();
            rc.call(
                SVC,
                "PutResourcePolicy",
                json!({"policyName": name, "policyDocument": document(p, "PolicyDocument")}),
            )
            .await
            .map_err(|e| rc.fail(SVC, &e))?;
            Ok(Made::new(name))
        }
        "AWS::Logs::QueryDefinition" => {
            let made = rc
                .call(
                    SVC,
                    "PutQueryDefinition",
                    json!({"name": text(p, "Name"), "queryString": text(p, "QueryString"),
                    "logGroupNames": p.get("LogGroupNames")}),
                )
                .await
                .map_err(|e| rc.fail(SVC, &e))?;
            let id = made
                .at("queryDefinitionId")
                .as_str()
                .unwrap_or_default()
                .to_owned();
            Ok(Made::new(&id).with("QueryDefinitionId", id.clone()))
        }
        other => Err(format!("{other} is not a type doze-aws makes")),
    }
}

async fn group(rc: &Rc, p: &Value) -> Done {
    let name = text(p, "LogGroupName").unwrap_or_else(|| super::generated_name(rc, 512, false));
    let found = rc
        .call(
            SVC,
            "DescribeLogGroups",
            json!({"logGroupNamePrefix": name}),
        )
        .await;
    if found.is_ok_and(|f| {
        f.at("logGroups")
            .as_array()
            .is_some_and(|a| a.iter().any(|g| g.at("logGroupName") == name.as_str()))
    }) {
        return Err(rc.taken(&name));
    }
    let mut body = json!({"logGroupName": name});
    if let Some(k) = text(p, "KmsKeyId") {
        body.put("kmsKeyId", k.into());
    }
    if let Some(c) = text(p, "LogGroupClass") {
        body.put("logGroupClass", c.into());
    }
    let tags: Map<String, Value> = tag_list(rc, p)
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();
    if !tags.is_empty() {
        body.put("tags", Value::Object(tags));
    }
    rc.call(SVC, "CreateLogGroup", body)
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    retention(rc, &name, &Value::Null, p).await?;
    Ok(Made::new(&name).with("Arn", group_arn(rc, &name)))
}

async fn retention(rc: &Rc, name: &str, before: &Value, p: &Value) -> Result<(), String> {
    let (then, now) = (text(before, "RetentionInDays"), text(p, "RetentionInDays"));
    if then == now {
        return Ok(());
    }
    let r = match now {
        Some(days) => {
            rc.call(
                SVC,
                "PutRetentionPolicy",
                json!({"logGroupName": name, "retentionInDays": days}),
            )
            .await
        }
        None => {
            rc.call(SVC, "DeleteRetentionPolicy", json!({"logGroupName": name}))
                .await
        }
    };
    r.map(|_| ()).map_err(|e| rc.fail(SVC, &e))
}

async fn put_metric_filter(rc: &Rc, name: &str, p: &Value) -> Result<(), String> {
    rc.call(
        SVC,
        "PutMetricFilter",
        json!({"logGroupName": text(p, "LogGroupName"), "filterName": name,
        "filterPattern": text(p, "FilterPattern").unwrap_or_default(),
        "metricTransformations": camel(p.get("MetricTransformations").unwrap_or(&Value::Null))}),
    )
    .await
    .map(|_| ())
    .map_err(|e| rc.fail(SVC, &e))
}

async fn put_subscription(rc: &Rc, name: &str, p: &Value) -> Result<(), String> {
    let mut body = json!({"logGroupName": text(p, "LogGroupName"), "filterName": name,
        "filterPattern": text(p, "FilterPattern").unwrap_or_default(), "destinationArn": text(p, "DestinationArn")});
    for (k, w) in [("RoleArn", "roleArn"), ("Distribution", "distribution")] {
        if let Some(v) = text(p, k) {
            body.put(w, v.into());
        }
    }
    rc.call(SVC, "PutSubscriptionFilter", body)
        .await
        .map(|_| ())
        .map_err(|e| rc.fail(SVC, &e))
}

pub(super) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    let physical = old.physical.clone().unwrap_or_default();
    let before = old.props_value();
    match rc.ty.as_str() {
        "AWS::Logs::LogGroup" => {
            retention(rc, &physical, &before, p).await?;
            let (then, now) = (tag_list(rc, &before), tag_list(rc, p));
            if then != now {
                let arn = group_arn(rc, &physical);
                let arn = arn.trim_end_matches(":*");
                let removed: Vec<&String> = then
                    .iter()
                    .map(|(k, _)| k)
                    .filter(|k| !now.iter().any(|(n, _)| n == *k))
                    .collect();
                if !removed.is_empty() {
                    drop(
                        rc.call(
                            SVC,
                            "UntagResource",
                            json!({"resourceArn": arn, "tagKeys": removed}),
                        )
                        .await,
                    );
                }
                let tags: Map<String, Value> = now
                    .into_iter()
                    .map(|(k, v)| (k, Value::String(v)))
                    .collect();
                if !tags.is_empty() {
                    rc.call(
                        SVC,
                        "TagResource",
                        json!({"resourceArn": arn, "tags": tags}),
                    )
                    .await
                    .map_err(|e| rc.fail(SVC, &e))?;
                }
            }
            return Ok(Made::new(&physical).with("Arn", group_arn(rc, &physical)));
        }
        "AWS::Logs::MetricFilter" => put_metric_filter(rc, &physical, p).await?,
        "AWS::Logs::SubscriptionFilter" => put_subscription(rc, &physical, p).await?,
        _ => return create(rc, p).await,
    }
    Ok(Made::new(physical))
}

pub(super) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    let physical = res.physical.clone().unwrap_or_default();
    let p = res.props_value();
    let (op, body) = match rc.ty.as_str() {
        "AWS::Logs::LogGroup" => ("DeleteLogGroup", json!({"logGroupName": physical})),
        "AWS::Logs::LogStream" => (
            "DeleteLogStream",
            json!({"logGroupName": text(&p, "LogGroupName"), "logStreamName": physical}),
        ),
        "AWS::Logs::MetricFilter" => (
            "DeleteMetricFilter",
            json!({"logGroupName": text(&p, "LogGroupName"), "filterName": physical}),
        ),
        "AWS::Logs::SubscriptionFilter" => (
            "DeleteSubscriptionFilter",
            json!({"logGroupName": text(&p, "LogGroupName"), "filterName": physical}),
        ),
        "AWS::Logs::ResourcePolicy" => ("DeleteResourcePolicy", json!({"policyName": physical})),
        "AWS::Logs::QueryDefinition" => (
            "DeleteQueryDefinition",
            json!({"queryDefinitionId": physical}),
        ),
        _ => return Ok(()),
    };
    match rc.call(SVC, op, body).await {
        Ok(_) => Ok(()),
        Err(e) if gone(&e) => Ok(()),
        Err(e) => Err(rc.fail(SVC, &e)),
    }
}
