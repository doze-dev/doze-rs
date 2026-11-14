//! The rest of the template (`template.rs`): functions and what invokes them, rules, keys, secrets, parameters and
//! state machines.

use serde_json::{Map, Value, json};

use super::spec::{Destination, Function, Key, Parameter, Rule, Secret, StateMachine};
use super::template::{Resources, arn, id, put, resource, tags};

fn destination(d: &Destination) -> Option<Value> {
    if let Some(q) = &d.queue {
        Some(arn("Queue", q))
    } else if let Some(t) = &d.topic {
        Some(json!({ "Ref": id("Topic", t) }))
    } else {
        d.lambda.as_ref().map(|f| arn("Function", f))
    }
}

/// The function's code: a local directory, read where it is (`S3Bucket: _local_`), relative to doze.hcl.
pub(crate) fn code_dir(code: &str, dir: &str) -> String {
    let p = std::path::Path::new(code);
    if p.is_absolute() || dir.is_empty() {
        code.to_owned()
    } else {
        std::path::Path::new(dir)
            .join(p)
            .to_string_lossy()
            .into_owned()
    }
}

pub(crate) fn function(r: &mut Resources, f: &Function, dir: &str) {
    let fid = id("Function", &f.name);
    let runtime = f
        .runtime
        .clone()
        .unwrap_or_else(|| "provided.al2023".into());
    let mut p = Map::new();
    p.insert("FunctionName".into(), f.name.clone().into());
    p.insert("Runtime".into(), runtime.into());
    p.insert(
        "Handler".into(),
        f.handler
            .clone()
            .unwrap_or_else(|| "bootstrap".into())
            .into(),
    );
    p.insert(
        "Code".into(),
        json!({ "S3Bucket": "_local_", "S3Key": code_dir(&f.code, dir) }),
    );
    p.insert(
        "Role".into(),
        json!({ "Fn::Sub": "arn:aws:iam::${AWS::AccountId}:role/doze" }),
    );
    put(&mut p, "Timeout", f.timeout.map(Value::from));
    put(&mut p, "MemorySize", f.memory.map(Value::from));
    if !f.env.is_empty() {
        p.insert("Environment".into(), json!({ "Variables": f.env }));
    }
    if let Some(t) = f.dlq.first().and_then(destination) {
        p.insert("DeadLetterConfig".into(), json!({ "TargetArn": t }));
    }
    put(&mut p, "Tags", tags(&f.tags));
    r.insert(fid.clone(), resource("AWS::Lambda::Function", p));
    let ok = f.on_success.first().and_then(destination);
    let failed = f.on_failure.first().and_then(destination);
    if f.retries.is_some() || ok.is_some() || failed.is_some() {
        let mut c = Map::new();
        c.insert("FunctionName".into(), json!({ "Ref": fid }));
        c.insert("Qualifier".into(), "$LATEST".into());
        put(&mut c, "MaximumRetryAttempts", f.retries.map(Value::from));
        let mut dest = Map::new();
        put(
            &mut dest,
            "OnSuccess",
            ok.map(|d| json!({ "Destination": d })),
        );
        put(
            &mut dest,
            "OnFailure",
            failed.map(|d| json!({ "Destination": d })),
        );
        if !dest.is_empty() {
            c.insert("DestinationConfig".into(), Value::Object(dest));
        }
        r.insert(
            format!("{fid}Invoke"),
            resource("AWS::Lambda::EventInvokeConfig", c),
        );
    }
    for (i, t) in f.trigger.iter().enumerate() {
        let mut m = Map::new();
        m.insert("FunctionName".into(), json!({ "Ref": fid }));
        m.insert("EventSourceArn".into(), arn("Queue", &t.queue));
        put(&mut m, "BatchSize", t.batch.map(Value::from));
        put(&mut m, "Enabled", t.enabled.map(Value::from));
        r.insert(
            format!("{fid}Trigger{i}"),
            resource("AWS::Lambda::EventSourceMapping", m),
        );
    }
}

/// A rule's target, `"queue:name"`, `"topic:name"` or `"lambda:name"`, as the resource it names.
pub(crate) fn target(t: &str) -> Option<Value> {
    match t.split_once(':')? {
        ("queue", n) => Some(arn("Queue", n)),
        ("topic", n) => Some(json!({ "Ref": id("Topic", n) })),
        ("lambda", n) => Some(arn("Function", n)),
        _ => None,
    }
}

pub(crate) fn rule(r: &mut Resources, x: &Rule) {
    let mut p = Map::new();
    p.insert("Name".into(), x.name.clone().into());
    put(&mut p, "EventBusName", x.bus.clone().map(Value::from));
    put(
        &mut p,
        "EventPattern",
        x.pattern
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok()),
    );
    put(
        &mut p,
        "ScheduleExpression",
        x.schedule.clone().map(Value::from),
    );
    let on = x.enabled.unwrap_or(true);
    p.insert(
        "State".into(),
        if on { "ENABLED" } else { "DISABLED" }.into(),
    );
    let targets: Vec<Value> = x
        .targets
        .iter()
        .enumerate()
        .filter_map(|(i, t)| Some(json!({ "Id": format!("t{i}"), "Arn": target(t)? })))
        .collect();
    if !targets.is_empty() {
        p.insert("Targets".into(), targets.into());
    }
    r.insert(id("Rule", &x.name), resource("AWS::Events::Rule", p));
}

pub(crate) fn key(r: &mut Resources, k: &Key) {
    let mut p = Map::new();
    put(&mut p, "KeySpec", k.spec.clone().map(Value::from));
    put(&mut p, "KeyUsage", k.usage.clone().map(Value::from));
    put(
        &mut p,
        "Description",
        k.description.clone().map(Value::from),
    );
    if k.rotation {
        p.insert("EnableKeyRotation".into(), true.into());
    }
    put(&mut p, "Tags", tags(&k.tags));
    let kid = id("Key", &k.name);
    r.insert(kid.clone(), resource("AWS::KMS::Key", p));
    // The label is how the person finds it: an alias of that name.
    let mut a = Map::new();
    a.insert("AliasName".into(), format!("alias/{}", k.name).into());
    a.insert("TargetKeyId".into(), json!({ "Ref": kid }));
    r.insert(format!("{kid}Alias"), resource("AWS::KMS::Alias", a));
}

pub(crate) fn secret(r: &mut Resources, x: &Secret) {
    let mut p = Map::new();
    p.insert("Name".into(), x.name.clone().into());
    put(&mut p, "SecretString", x.value.clone().map(Value::from));
    put(
        &mut p,
        "Description",
        x.description.clone().map(Value::from),
    );
    put(&mut p, "Tags", tags(&x.tags));
    r.insert(
        id("Secret", &x.name),
        resource("AWS::SecretsManager::Secret", p),
    );
}

pub(crate) fn parameter(r: &mut Resources, x: &Parameter) {
    let mut p = Map::new();
    p.insert("Name".into(), x.name.clone().into());
    p.insert("Value".into(), x.value.clone().into());
    p.insert(
        "Type".into(),
        x.kind.clone().unwrap_or_else(|| "String".into()).into(),
    );
    put(
        &mut p,
        "Description",
        x.description.clone().map(Value::from),
    );
    if !x.tags.is_empty() {
        p.insert("Tags".into(), json!(x.tags));
    }
    r.insert(id("Parameter", &x.name), resource("AWS::SSM::Parameter", p));
}

pub(crate) fn state_machine(r: &mut Resources, m: &StateMachine) {
    let mut p = Map::new();
    p.insert("StateMachineName".into(), m.name.clone().into());
    p.insert("DefinitionString".into(), m.definition.clone().into());
    p.insert(
        "RoleArn".into(),
        m.role.clone().map_or_else(
            || json!({ "Fn::Sub": "arn:aws:iam::${AWS::AccountId}:role/stepfunctions" }),
            Value::from,
        ),
    );
    p.insert(
        "StateMachineType".into(),
        m.kind.clone().unwrap_or_else(|| "STANDARD".into()).into(),
    );
    put(&mut p, "Tags", tags(&m.tags));
    r.insert(
        id("StateMachine", &m.name),
        resource("AWS::StepFunctions::StateMachine", p),
    );
}
