//! A bucket's settings, read from the template's spelling (`CorsRules`, `ExpirationInDays`, `S3Key`...) into S3's (`CORSRules`,
//! `Expiration.Days`, `Key.FilterRules`...).

use serde_json::{Map, Value, json};

use crate::j::J;

fn get<'a>(p: &'a Value, key: &str) -> Option<&'a Value> {
    p.get(key).filter(|v| !v.is_null())
}

pub(super) fn public_access(p: &Value) -> Option<Value> {
    get(p, "PublicAccessBlockConfiguration").cloned()
}

pub(super) fn ownership(p: &Value) -> Option<Value> {
    get(p, "OwnershipControls").cloned()
}

pub(super) fn versioning(p: &Value) -> Option<Value> {
    get(p, "VersioningConfiguration").cloned()
}

pub(super) fn accelerate(p: &Value) -> Option<Value> {
    get(p, "AccelerateConfiguration").map(|a| json!({"Status": a.get("AccelerationStatus")}))
}

pub(super) fn encryption(p: &Value) -> Option<Value> {
    get(p, "BucketEncryption").map(|e| json!({"Rules": e.get("ServerSideEncryptionConfiguration").cloned().unwrap_or(json!([]))}))
}

pub(super) fn cors(p: &Value) -> Option<Value> {
    let rules = get(p, "CorsConfiguration")?.get("CorsRules")?.as_array()?;
    let rules: Vec<Value> = rules
        .iter()
        .map(|r| {
            let mut o = Map::new();
            for (from, to) in [
                ("AllowedHeaders", "AllowedHeaders"),
                ("AllowedMethods", "AllowedMethods"),
                ("AllowedOrigins", "AllowedOrigins"),
                ("ExposedHeaders", "ExposeHeaders"),
                ("Id", "ID"),
                ("MaxAge", "MaxAgeSeconds"),
            ] {
                if let Some(v) = r.get(from) {
                    o.insert(to.into(), v.clone());
                }
            }
            Value::Object(o)
        })
        .collect();
    Some(json!({"CORSRules": rules}))
}

/// A `{Name, Value}` filter written CloudFormation's way (`S3Key.Rules`) in S3's (`Key.FilterRules`).
fn key_filter(f: Option<&Value>) -> Option<Value> {
    let rules = f?.get("S3Key")?.get("Rules")?.clone();
    Some(json!({"Key": {"FilterRules": rules}}))
}

pub(super) fn lifecycle(p: &Value) -> Option<Value> {
    let rules = get(p, "LifecycleConfiguration")?.get("Rules")?.as_array()?;
    let rules: Vec<Value> = rules.iter().map(lifecycle_rule).collect();
    Some(json!({"Rules": rules}))
}

/// A lifecycle rule's filter: its prefix, tags and sizes, as one `Prefix` or an `And`.
fn lifecycle_filter(r: &Value) -> Value {
    let mut and = Map::new();
    if let Some(prefix) = r.get("Prefix") {
        and.insert("Prefix".into(), prefix.clone());
    }
    if let Some(tags) = r
        .get("TagFilters")
        .and_then(Value::as_array)
        .filter(|t| !t.is_empty())
    {
        and.insert("Tags".into(), Value::Array(tags.clone()));
    }
    for k in ["ObjectSizeGreaterThan", "ObjectSizeLessThan"] {
        if let Some(v) = r.get(k) {
            and.insert(k.into(), v.clone());
        }
    }
    match and.len() {
        0 => json!({"Prefix": ""}),
        1 if and.contains_key("Prefix") => json!({"Prefix": and.get("Prefix")}),
        _ => json!({"And": and}),
    }
}

fn lifecycle_rule(r: &Value) -> Value {
    let mut o = Map::new();
    o.insert(
        "Status".into(),
        r.get("Status").cloned().unwrap_or("Enabled".into()),
    );
    if let Some(id) = r.get("Id") {
        o.insert("ID".into(), id.clone());
    }
    o.insert("Filter".into(), lifecycle_filter(r));
    let mut exp = Map::new();
    if let Some(d) = r.get("ExpirationInDays") {
        exp.insert("Days".into(), d.clone());
    }
    if let Some(d) = r.get("ExpirationDate") {
        exp.insert("Date".into(), d.clone());
    }
    if let Some(d) = r.get("ExpiredObjectDeleteMarker") {
        exp.insert("ExpiredObjectDeleteMarker".into(), d.clone());
    }
    if !exp.is_empty() {
        o.insert("Expiration".into(), Value::Object(exp));
    }
    let noncurrent = r
        .get("NoncurrentVersionExpiration")
        .and_then(|n| n.get("NoncurrentDays"))
        .or_else(|| r.get("NoncurrentVersionExpirationInDays"));
    if let Some(d) = noncurrent {
        o.insert(
            "NoncurrentVersionExpiration".into(),
            json!({"NoncurrentDays": d}),
        );
    }
    if let Some(a) = r.get("AbortIncompleteMultipartUpload") {
        o.insert("AbortIncompleteMultipartUpload".into(), a.clone());
    }
    let transitions: Vec<Value> = r
        .get("Transitions")
        .and_then(Value::as_array)
        .cloned()
        .or_else(|| r.get("Transition").map(|t| vec![t.clone()]))
        .unwrap_or_default()
        .into_iter()
        .map(|t| json!({"Days": t.get("TransitionInDays"), "Date": t.get("TransitionDate"), "StorageClass": t.get("StorageClass")}))
        .map(|mut t| {
            if let Value::Object(o) = &mut t {
                o.retain(|_, v| !v.is_null());
            }
            t
        })
        .collect();
    if !transitions.is_empty() {
        o.insert("Transitions".into(), Value::Array(transitions));
    }
    Value::Object(o)
}

pub(super) fn website(p: &Value) -> Option<Value> {
    let w = get(p, "WebsiteConfiguration")?;
    let mut o = Map::new();
    if let Some(i) = w.get("IndexDocument") {
        o.insert("IndexDocument".into(), json!({"Suffix": i}));
    }
    if let Some(e) = w.get("ErrorDocument") {
        o.insert("ErrorDocument".into(), json!({"Key": e}));
    }
    if let Some(r) = w.get("RedirectAllRequestsTo") {
        o.insert("RedirectAllRequestsTo".into(), r.clone());
    }
    if let Some(rules) = w.get("RoutingRules").and_then(Value::as_array) {
        let rules: Vec<Value> = rules
            .iter()
            .map(|r| {
                let mut x =
                    json!({"Redirect": r.get("RedirectRule").cloned().unwrap_or(json!({}))});
                if let Some(c) = r.get("RoutingRuleCondition") {
                    x.put("Condition", c.clone());
                }
                x
            })
            .collect();
        o.insert("RoutingRules".into(), Value::Array(rules));
    }
    Some(Value::Object(o))
}

pub(super) fn logging(p: &Value) -> Option<Value> {
    let l = get(p, "LoggingConfiguration")?;
    let mut enabled = json!({"TargetBucket": l.get("DestinationBucketName"), "TargetPrefix": l.get("LogFilePrefix").cloned().unwrap_or("".into())});
    if enabled.at("TargetBucket").is_null() {
        enabled.put("TargetBucket", Value::Null);
    }
    Some(json!({"LoggingEnabled": enabled}))
}

pub(super) fn object_lock(p: &Value) -> Option<Value> {
    let c = get(p, "ObjectLockConfiguration")?;
    let mut o = json!({"ObjectLockEnabled": "Enabled"});
    if let Some(rule) = c.get("Rule") {
        o.put("Rule", rule.clone());
    }
    Some(o)
}

pub(super) fn replication(p: &Value) -> Option<Value> {
    let r = get(p, "ReplicationConfiguration")?;
    let rules: Vec<Value> = r
        .get("Rules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|mut rule| {
            if let Some(o) = rule.as_object_mut()
                && let Some(id) = o.remove("Id")
            {
                o.insert("ID".into(), id);
            }
            rule
        })
        .collect();
    Some(json!({"Role": r.get("Role"), "Rules": rules}))
}

pub(super) fn notifications(p: &Value) -> Option<Value> {
    let n = get(p, "NotificationConfiguration")?;
    let mut o = Map::new();
    let list = |key: &str, target: &str, arn: &str| -> Vec<Value> {
        n.get(key)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|c| {
                        let mut x = json!({arn: c.get(target), "Events": [c.get("Event")]});
                        if let Some(f) = key_filter(c.get("Filter")) {
                            x.put("Filter", f);
                        }
                        x
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    for (key, target, out, arn) in [
        (
            "QueueConfigurations",
            "Queue",
            "QueueConfigurations",
            "QueueArn",
        ),
        (
            "TopicConfigurations",
            "Topic",
            "TopicConfigurations",
            "TopicArn",
        ),
        (
            "LambdaConfigurations",
            "Function",
            "LambdaFunctionConfigurations",
            "LambdaFunctionArn",
        ),
    ] {
        let items = list(key, target, arn);
        if !items.is_empty() {
            o.insert(out.into(), Value::Array(items));
        }
    }
    if n.get("EventBridgeConfiguration")
        .and_then(|e| e.get("EventBridgeEnabled"))
        .is_some_and(|v| v.as_bool() == Some(true) || v.as_str() == Some("true"))
    {
        o.insert("EventBridgeConfiguration".into(), json!({}));
    }
    Some(Value::Object(o))
}
