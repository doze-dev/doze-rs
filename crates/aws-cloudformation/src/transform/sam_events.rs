//! What a SAM function brings with it: its execution role (with SAM's managed policies and policy templates), its URL, version and
//! alias, its event invoke configuration, and its events; and SAM state machines.

use serde_json::{Map, Value, json};

use aws_wire::ApiError;

use super::sam::resource;
use crate::errors::validation;
use crate::j::J;
use crate::tmpl::eval::Pseudo;

/// What the expansion makes.
#[derive(Debug, Default)]
pub(crate) struct Out {
    pub(crate) resources: Map<String, Value>,
    /// `Ref` targets SAM rewrites (`Fn.Alias` to the alias's logical id).
    pub(crate) aliases: Map<String, Value>,
    /// Routes of REST APIs: API logical id (or `ServerlessRestApi`), path, method, function, event.
    pub(crate) rest_routes: Vec<(String, String, String, String, String)>,
    /// Routes of HTTP APIs, likewise (`ServerlessHttpApi` for the implicit one).
    pub(crate) http_routes: Vec<(String, String, String, String, String)>,
}

impl Out {
    pub(crate) fn put(&mut self, name: &str, v: Value) {
        self.resources.insert(name.to_owned(), v);
    }
}

/// A function's URL, version and alias, and event invoke configuration.
pub(crate) fn function_extras(name: &str, p: &Value, r: &Value, out: &mut Out, _pseudo: &Pseudo) {
    if let Some(url) = p.get("FunctionUrlConfig") {
        let mut u = url.clone();
        u.put("TargetFunctionArn", json!({"Ref": name}));
        out.put(&format!("{name}Url"), resource("AWS::Lambda::Url", u, r));
        if url.at("AuthType") == "NONE" {
            out.put(&format!("{name}UrlPublicPermissions"), resource("AWS::Lambda::Permission",
                json!({"Action": "lambda:InvokeFunctionUrl", "FunctionName": {"Ref": name}, "Principal": "*", "FunctionUrlAuthType": "NONE"}), r));
        }
    }
    if let Some(alias) = p.get("AutoPublishAlias").and_then(Value::as_str) {
        let version = format!("{name}Version{}", super::sam::hash10(p));
        // SAM keeps old versions: a new one replaces the logical id when the code changes, and the old is retained.
        let mut v = resource(
            "AWS::Lambda::Version",
            json!({"FunctionName": {"Ref": name}}),
            r,
        );
        v.put("DeletionPolicy", "Retain".into());
        out.put(&version, v);
        let alias_id = format!(
            "{name}Alias{}",
            alias
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .collect::<String>()
        );
        let mut a = json!({"Name": alias, "FunctionName": {"Ref": name}, "FunctionVersion": {"Fn::GetAtt": [version, "Version"]}});
        if let Some(pc) = p.get("ProvisionedConcurrencyConfig") {
            a.put("ProvisionedConcurrencyConfig", pc.clone());
        }
        out.put(&alias_id, resource("AWS::Lambda::Alias", a, r));
        out.aliases.insert(format!("{name}.Alias"), alias_id.into());
        out.aliases
            .insert(format!("{name}.Version"), version.into());
    }
    if let Some(cfg) = p.get("EventInvokeConfig") {
        let mut c = json!({"FunctionName": {"Ref": name}, "Qualifier": "$LATEST"});
        for k in [
            "MaximumEventAgeInSeconds",
            "MaximumRetryAttempts",
            "DestinationConfig",
        ] {
            if let Some(v) = cfg.get(k) {
                c.put(k, v.clone());
            }
        }
        out.put(
            &format!("{name}EventInvokeConfig"),
            resource("AWS::Lambda::EventInvokeConfig", c, r),
        );
    }
}

/// A permission for `principal` to invoke function `name`.
fn permission(name: &str, principal: &str, source: Option<Value>, r: &Value) -> Value {
    let mut p = json!({"Action": "lambda:InvokeFunction", "FunctionName": {"Ref": name}, "Principal": principal});
    if let Some(s) = source {
        p.put("SourceArn", s);
    }
    resource("AWS::Lambda::Permission", p, r)
}

pub(crate) fn pick(e: &Value, keys: &[&str]) -> Value {
    let mut out = json!({});
    for k in keys {
        if let Some(v) = e.get(*k) {
            out.put(k, v.clone());
        }
    }
    out
}

const MAPPING_KEYS: &[&str] = &[
    "BatchSize",
    "Enabled",
    "FilterCriteria",
    "MaximumBatchingWindowInSeconds",
    "FunctionResponseTypes",
    "ScalingConfig",
    "StartingPosition",
    "StartingPositionTimestamp",
    "MaximumRetryAttempts",
    "BisectBatchOnFunctionError",
    "MaximumRecordAgeInSeconds",
    "DestinationConfig",
    "ParallelizationFactor",
    "TumblingWindowInSeconds",
    "KmsKeyArn",
    "MetricsConfig",
];

/// An SNS event: a subscription of the function to the topic, and the topic's permission to invoke it.
fn sns_event(name: &str, id: &str, props: &Value, target: &Value, r: &Value, out: &mut Out) {
    let mut s = json!({"Protocol": "lambda", "Endpoint": target, "TopicArn": props.at("Topic")});
    for k in [
        "FilterPolicy",
        "FilterPolicyScope",
        "Region",
        "RedrivePolicy",
    ] {
        if let Some(v) = props.get(k) {
            s.put(k, v.clone());
        }
    }
    out.put(id, resource("AWS::SNS::Subscription", s, r));
    let permission = permission(
        name,
        "sns.amazonaws.com",
        Some(props.at("Topic").clone()),
        r,
    );
    out.put(&format!("{id}Permission"), permission);
}

/// The resources a function's `Events` make.
pub(crate) fn function_events(
    name: &str,
    p: &Value,
    r: &Value,
    all: &Map<String, Value>,
    out: &mut Out,
) -> Result<(), ApiError> {
    let Some(events) = p.get("Events").and_then(Value::as_object) else {
        return Ok(());
    };
    let target = match p.get("AutoPublishAlias") {
        Some(_) => json!({"Ref": format!("{name}.Alias")}),
        None => json!({"Fn::GetAtt": [name, "Arn"]}),
    };
    for (ev, e) in events {
        let id = format!("{name}{ev}");
        let props = e.get("Properties").cloned().unwrap_or(json!({}));
        let ty = e.at("Type").as_str().unwrap_or_default();
        match ty {
            "SQS" | "Kinesis" | "DynamoDB" => {
                let source = props
                    .get("Queue")
                    .or_else(|| props.get("Stream"))
                    .cloned()
                    .unwrap_or(Value::Null);
                let mut m = pick(&props, MAPPING_KEYS);
                m.put("EventSourceArn", source);
                m.put("FunctionName", target.clone());
                out.put(&id, resource("AWS::Lambda::EventSourceMapping", m, r));
            }
            "SNS" => sns_event(name, &id, &props, &target, r, out),
            "S3" => s3_event((name, &id), &props, &target, r, all, out)?,
            "Schedule" | "ScheduleV2" | "EventBridgeRule" | "CloudWatchEvent" => {
                rule_event(name, &id, &props, &target, r, out)
            }
            "CloudWatchLogs" => {
                let f = json!({"LogGroupName": props.at("LogGroupName"), "FilterPattern": props.get("FilterPattern").cloned().unwrap_or("".into()), "DestinationArn": target});
                let perm = format!("{id}Permission");
                out.put(&perm, permission(name, "logs.amazonaws.com", None, r));
                let mut sub = resource("AWS::Logs::SubscriptionFilter", f, r);
                sub.put("DependsOn", json!([perm]));
                out.put(&id, sub);
            }
            "Api" => {
                let api = api_of(&props, "RestApiId", "ServerlessRestApi");
                route(&mut out.rest_routes, &api, &props, name, ev);
                let source = json!({"Fn::Sub": format!("arn:${{AWS::Partition}}:execute-api:${{AWS::Region}}:${{AWS::AccountId}}:${{{api}}}/*/*")});
                out.put(
                    &format!("{id}Permission"),
                    permission(name, "apigateway.amazonaws.com", Some(source), r),
                );
            }
            "HttpApi" => {
                let api = api_of(&props, "ApiId", "ServerlessHttpApi");
                route(&mut out.http_routes, &api, &props, name, ev);
                let source = json!({"Fn::Sub": format!("arn:${{AWS::Partition}}:execute-api:${{AWS::Region}}:${{AWS::AccountId}}:${{{api}}}/*")});
                out.put(
                    &format!("{id}Permission"),
                    permission(name, "apigateway.amazonaws.com", Some(source), r),
                );
            }
            other => {
                return Err(validation(format!(
                    "Transform AWS::Serverless-2016-10-31 failed with: Event {ev} of function {name}: event type {other} is not served by doze-aws"
                )));
            }
        }
    }
    Ok(())
}

fn api_of(props: &Value, key: &str, implicit: &str) -> String {
    props
        .get(key)
        .and_then(|v| v.get("Ref").and_then(Value::as_str).or(v.as_str()))
        .unwrap_or(implicit)
        .to_owned()
}

fn route(
    routes: &mut Vec<(String, String, String, String, String)>,
    api: &str,
    props: &Value,
    function: &str,
    ev: &str,
) {
    let path = props.at("Path").as_str().unwrap_or("$default").to_owned();
    let method = props
        .at("Method")
        .as_str()
        .unwrap_or("any")
        .to_ascii_lowercase();
    routes.push((
        api.to_owned(),
        path,
        method,
        function.to_owned(),
        ev.to_owned(),
    ));
}

/// An S3 event: the function goes into the bucket's notification configuration (the bucket is in the same template, as SAM requires).
fn s3_event(
    (name, id): (&str, &str),
    props: &Value,
    target: &Value,
    r: &Value,
    all: &Map<String, Value>,
    out: &mut Out,
) -> Result<(), ApiError> {
    let Some(bucket) = props.at("Bucket").get("Ref").and_then(Value::as_str) else {
        return Err(validation(format!(
            "Transform AWS::Serverless-2016-10-31 failed with: Event {id}: the S3 Bucket must be a Ref to a bucket in this template"
        )));
    };
    let perm = format!("{id}Permission");
    out.put(&perm, permission(name, "s3.amazonaws.com", None, r));
    let mut b = out
        .resources
        .get(bucket)
        .cloned()
        .or_else(|| all.get(bucket).cloned())
        .unwrap_or(json!({"Type": "AWS::S3::Bucket"}));
    let events = match props.get("Events") {
        Some(Value::Array(a)) => a.clone(),
        Some(one) => vec![one.clone()],
        None => Vec::new(),
    };
    let mut lambdas: Vec<Value> = b
        .at("Properties")
        .at("NotificationConfiguration")
        .at("LambdaConfigurations")
        .as_array()
        .cloned()
        .unwrap_or_default();
    for event in events {
        let mut c = json!({"Event": event, "Function": target});
        if let Some(f) = props.get("Filter") {
            c.put("Filter", f.clone());
        }
        lambdas.push(c);
    }
    let mut p = b.get("Properties").cloned().unwrap_or(json!({}));
    let mut n = p
        .get("NotificationConfiguration")
        .cloned()
        .unwrap_or(json!({}));
    n.put("LambdaConfigurations", Value::Array(lambdas));
    p.put("NotificationConfiguration", n);
    b.put("Properties", p);
    let mut deps = match b.get("DependsOn") {
        Some(Value::Array(a)) => a.clone(),
        Some(one) => vec![one.clone()],
        None => Vec::new(),
    };
    deps.push(perm.into());
    b.put("DependsOn", Value::Array(deps));
    out.put(bucket, b);
    Ok(())
}

/// A schedule or EventBridge rule targeting `target`. `ScheduleV2` is made as a rule too (EventBridge Scheduler is not served).
pub(crate) fn rule_event(
    name: &str,
    id: &str,
    props: &Value,
    target: &Value,
    r: &Value,
    out: &mut Out,
) {
    let mut rule = pick(
        props,
        &["Description", "Name", "EventBusName", "State", "RoleArn"],
    );
    if let Some(s) = props
        .get("Schedule")
        .or_else(|| props.get("ScheduleExpression"))
    {
        rule.put("ScheduleExpression", s.clone());
    }
    if let Some(pattern) = props.get("Pattern") {
        rule.put("EventPattern", pattern.clone());
    }
    if rule.get("State").is_none() {
        let enabled = props
            .get("Enabled")
            .is_none_or(|v| v.as_bool() != Some(false));
        rule.put("State", if enabled { "ENABLED" } else { "DISABLED" }.into());
    }
    let mut t = pick(
        props,
        &[
            "Input",
            "InputPath",
            "InputTransformer",
            "DeadLetterConfig",
            "RetryPolicy",
        ],
    );
    t.put("Arn", target.clone());
    let target_id = props
        .at("Target")
        .at("Id")
        .as_str()
        .map_or_else(|| format!("{id}LambdaTarget"), str::to_owned);
    t.put("Id", target_id.into());
    rule.put("Targets", json!([t]));
    out.put(id, resource("AWS::Events::Rule", rule, r));
    out.put(
        &format!("{id}Permission"),
        permission(
            name,
            "events.amazonaws.com",
            Some(json!({"Fn::GetAtt": [id, "Arn"]})),
            r,
        ),
    );
}
