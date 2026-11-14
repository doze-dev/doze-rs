//! `AWS::Serverless::StateMachine`: the state machine, its role, and its schedule and EventBridge events (each a rule whose target
//! starts an execution through a role of its own, as SAM makes them).

use serde_json::{Value, json};

use aws_wire::ApiError;

use super::sam::{resource, tag_list};
use super::sam_events::{Out, pick, rule_event};
use crate::errors::validation;
use crate::j::J;

/// The state machine's own properties.
fn machine(name: &str, p: &Value) -> Result<Value, ApiError> {
    let mut m = pick(
        p,
        &[
            "DefinitionSubstitutions",
            "LoggingConfiguration",
            "TracingConfiguration",
        ],
    );
    if let Some(d) = p.get("Definition") {
        m.put("Definition", d.clone());
    }
    if let Some(uri) = p.get("DefinitionUri") {
        let loc = super::sam::location(name, Some(uri), "DefinitionUri")?;
        m.put(
            "DefinitionS3Location",
            json!({"Bucket": loc.at("S3Bucket"), "Key": loc.at("S3Key")}),
        );
    }
    for (from, to) in [
        ("Name", "StateMachineName"),
        ("Type", "StateMachineType"),
        ("Logging", "LoggingConfiguration"),
        ("Tracing", "TracingConfiguration"),
    ] {
        if let Some(v) = p.get(from) {
            m.put(to, v.clone());
        }
    }
    let tags = tag_list(p, false);
    if !tags.is_empty() {
        m.put("Tags", Value::Array(tags));
    }
    Ok(m)
}

/// `AWS::Serverless::StateMachine` as a state machine, its role, and its events.
///
/// # Errors
/// An event type doze-aws does not serve, or a policy SAM refuses.
pub(crate) fn state_machine(name: &str, r: &Value, out: &mut Out) -> Result<(), ApiError> {
    let p = r.get("Properties").cloned().unwrap_or(json!({}));
    let mut m = machine(name, &p)?;
    let role = match p.get("Role") {
        Some(role) => role.clone(),
        None => {
            out.put(
                &format!("{name}Role"),
                super::sam_role::execution_role(&p, r, "states.amazonaws.com")?,
            );
            json!({"Fn::GetAtt": [format!("{name}Role"), "Arn"]})
        }
    };
    m.put("RoleArn", role);
    out.put(name, resource("AWS::StepFunctions::StateMachine", m, r));
    for (ev, e) in p
        .get("Events")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        event(name, ev, e, r, out)?;
    }
    Ok(())
}

/// One event of a state machine: a rule, and the role its target starts executions with.
fn event(name: &str, ev: &str, e: &Value, r: &Value, out: &mut Out) -> Result<(), ApiError> {
    let ty = e.at("Type").as_str().unwrap_or_default();
    if !matches!(
        ty,
        "Schedule" | "ScheduleV2" | "EventBridgeRule" | "CloudWatchEvent"
    ) {
        return Err(validation(format!(
            "Transform AWS::Serverless-2016-10-31 failed with: Event {ev} of state machine {name}: event type {ty} is not served by doze-aws"
        )));
    }
    let id = format!("{name}{ev}");
    let role_id = format!("{id}Role");
    let doc = json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Action": "states:StartExecution", "Resource": {"Ref": name}}]});
    let trust = json!({"Version": "2012-10-17", "Statement": [
        {"Effect": "Allow", "Action": ["sts:AssumeRole"], "Principal": {"Service": ["events.amazonaws.com"]}}]});
    let role = json!({"AssumeRolePolicyDocument": trust,
        "Policies": [{"PolicyName": format!("{id}RoleStartExecutionPolicy"), "PolicyDocument": doc}]});
    out.put(&role_id, resource("AWS::IAM::Role", role, r));
    let props = e.get("Properties").cloned().unwrap_or(json!({}));
    rule_event(name, &id, &props, &json!({"Ref": name}), r, out);
    out.resources.remove(&format!("{id}Permission"));
    if let Some(rule) = out.resources.get_mut(&id)
        && let Some(Value::Array(ts)) = rule
            .get_mut("Properties")
            .and_then(|p| p.get_mut("Targets"))
        && let Some(t) = ts.first_mut()
    {
        t.put("RoleArn", json!({"Fn::GetAtt": [role_id, "Arn"]}));
        t.put("Id", format!("{id}StepFunctionsTarget").into());
    }
    Ok(())
}
