//! The execution role SAM makes for a function or a state machine: the service in its trust policy, SAM's managed policies (basic
//! execution, X-Ray, the poller policies of the function's event sources), and the template's `Policies` (managed policy names and
//! ARNs, documents, and SAM policy templates).

use serde_json::{Value, json};

use aws_wire::ApiError;

use super::sam::{resource, tag_list};
use crate::errors::validation;
use crate::j::J;

fn managed(name: &str) -> Value {
    json!({"Fn::Sub": format!("arn:${{AWS::Partition}}:iam::aws:policy/{name}")})
}

/// The managed policies SAM attaches by itself.
fn own_policies(p: &Value, service: &str) -> Vec<Value> {
    let mut arns = Vec::new();
    if service != "lambda.amazonaws.com" {
        return arns;
    }
    arns.push(managed("service-role/AWSLambdaBasicExecutionRole"));
    if p.at("Tracing") == "Active" {
        arns.push(managed("AWSXrayWriteOnlyAccess"));
    }
    for e in p
        .get("Events")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(_, e)| e)
    {
        let extra = match e.at("Type").as_str() {
            Some("SQS") => Some("service-role/AWSLambdaSQSQueueExecutionRole"),
            Some("Kinesis") => Some("service-role/AWSLambdaKinesisExecutionRole"),
            Some("DynamoDB") => Some("service-role/AWSLambdaDynamoDBExecutionRole"),
            _ => None,
        };
        if let Some(x) = extra {
            arns.push(managed(x));
        }
    }
    arns
}

/// The template's `Policies`, split into managed policy ARNs and inline documents.
fn given_policies(p: &Value, arns: &mut Vec<Value>) -> Result<Vec<Value>, ApiError> {
    let policies = match p.get("Policies") {
        Some(Value::Array(a)) => a.clone(),
        Some(one) => vec![one.clone()],
        None => Vec::new(),
    };
    let mut inline = Vec::new();
    for (i, pol) in policies.iter().enumerate() {
        match pol {
            Value::String(s) if s.starts_with("arn:") => arns.push(pol.clone()),
            Value::String(s) => arns.push(managed(s)),
            Value::Object(o) if o.contains_key("Statement") => {
                inline.push(policy_doc(i, pol.clone()))
            }
            Value::Object(o)
                if o.len() == 1 && !o.contains_key("Ref") && !o.contains_key("Fn::If") =>
            {
                let (template, args) = o
                    .iter()
                    .next()
                    .map_or(("", &Value::Null), |(k, v)| (k.as_str(), v));
                inline.push(policy_doc(
                    i,
                    super::sam_policies::template(template, args)?,
                ));
            }
            other if other.get("Fn::If").is_some() || other.get("Ref").is_some() => {
                arns.push(other.clone())
            }
            _ => {
                return Err(validation(
                    "Transform AWS::Serverless-2016-10-31 failed with: a policy is not a name, an ARN, a document or a policy template",
                ));
            }
        }
    }
    Ok(inline)
}

/// The role SAM makes for a function or a state machine.
///
/// # Errors
/// A policy SAM refuses, or a policy template doze-aws does not write.
pub(crate) fn execution_role(p: &Value, r: &Value, service: &str) -> Result<Value, ApiError> {
    let mut arns = own_policies(p, service);
    let inline = given_policies(p, &mut arns)?;
    let trust = json!({"Version": "2012-10-17", "Statement": [
        {"Effect": "Allow", "Action": ["sts:AssumeRole"], "Principal": {"Service": [service]}}]});
    let mut props = json!({"AssumeRolePolicyDocument": trust, "ManagedPolicyArns": arns,
        "Tags": tag_list(p, service == "lambda.amazonaws.com")});
    if !inline.is_empty() {
        props.put("Policies", Value::Array(inline));
    }
    if let Some(v) = p.get("PermissionsBoundary") {
        props.put("PermissionsBoundary", v.clone());
    }
    if let Some(v) = p.get("RolePath") {
        props.put("Path", v.clone());
    }
    let mut role = resource("AWS::IAM::Role", props, r);
    if let Some(o) = role.as_object_mut() {
        o.remove("DependsOn");
    }
    Ok(role)
}

/// SAM names inline policies after the function's role (memory: `<Role>Policy<n>`); a plain sequence here.
fn policy_doc(i: usize, mut doc: Value) -> Value {
    if doc.get("Version").is_none() {
        doc.put("Version", "2012-10-17".into());
    }
    json!({"PolicyName": format!("Policy{i}"), "PolicyDocument": doc})
}
