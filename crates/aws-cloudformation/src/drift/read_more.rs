//! Readers of the types beyond the first five: DynamoDB tables, IAM roles, Lambda functions, Secrets Manager secrets, KMS keys.

use serde_json::{Value, json};

use super::read::{Live, Probe, Read, found, pick};
use crate::j::J;

pub(super) async fn read(p: &Probe, ty: &str, physical: &str) -> Read {
    match ty {
        "AWS::DynamoDB::Table" => table(p, physical).await,
        "AWS::IAM::Role" => role(p, physical).await,
        "AWS::Lambda::Function" => function(p, physical).await,
        "AWS::SecretsManager::Secret" => secret(p, physical).await,
        "AWS::KMS::Key" => key(p, physical).await,
        other => Err(format!("{other} cannot be read back")),
    }
}

async fn table(p: &Probe, name: &str) -> Read {
    let got = p
        .call("dynamodb", "DescribeTable", json!({"TableName": name}))
        .await;
    let Some(v) = found(got)? else {
        return Ok(Live::Gone);
    };
    let t = v.at("Table");
    let mut out = pick(
        t,
        &[
            ("TableName", "TableName"),
            ("KeySchema", "KeySchema"),
            ("AttributeDefinitions", "AttributeDefinitions"),
        ],
    );
    let billing = t
        .at("BillingModeSummary")
        .at("BillingMode")
        .as_str()
        .unwrap_or("PROVISIONED")
        .to_owned();
    if billing == "PROVISIONED" {
        let pt = t.at("ProvisionedThroughput");
        out.insert(
            "ProvisionedThroughput".into(),
            json!({"ReadCapacityUnits": pt.at("ReadCapacityUnits"), "WriteCapacityUnits": pt.at("WriteCapacityUnits")}),
        );
    } else {
        out.insert("ProvisionedThroughput".into(), Value::Null);
    }
    out.insert("BillingMode".into(), billing.into());
    let stream = t.at("StreamSpecification");
    out.insert(
        "StreamSpecification".into(),
        if stream.at("StreamEnabled").as_bool() == Some(true) {
            json!({"StreamViewType": stream.at("StreamViewType")})
        } else {
            Value::Null
        },
    );
    out.insert(
        "TableClass".into(),
        t.at("TableClassSummary")
            .get("TableClass")
            .cloned()
            .unwrap_or_else(|| "STANDARD".into()),
    );
    out.insert(
        "DeletionProtectionEnabled".into(),
        t.get("DeletionProtectionEnabled")
            .cloned()
            .unwrap_or(Value::Bool(false)),
    );
    Ok(Live::Found(out))
}

/// IAM gives a policy document URL-encoded (as AWS does); the comparison reads it as JSON.
fn decoded(v: &Value) -> Value {
    let Some(s) = v.as_str() else {
        return v.clone();
    };
    let text = if s.starts_with('%') || s.contains("%22") {
        aws_core::uri::decode_str(s).map_or_else(|| s.to_owned(), |d| d.into_owned())
    } else {
        s.to_owned()
    };
    serde_json::from_str(&text).unwrap_or(Value::String(text))
}

async fn role(p: &Probe, name: &str) -> Read {
    let got = p.call("iam", "GetRole", json!({"RoleName": name})).await;
    let Some(v) = found(got)? else {
        return Ok(Live::Gone);
    };
    let r = v.at("Role");
    let mut out = pick(
        r,
        &[
            ("RoleName", "RoleName"),
            ("Path", "Path"),
            ("Description", "Description"),
            ("MaxSessionDuration", "MaxSessionDuration"),
        ],
    );
    out.insert(
        "AssumeRolePolicyDocument".into(),
        decoded(r.at("AssumeRolePolicyDocument")),
    );
    let attached = p
        .call("iam", "ListAttachedRolePolicies", json!({"RoleName": name}))
        .await
        .map_err(|e| e.message)?;
    let arns: Vec<Value> = attached
        .at("AttachedPolicies")
        .as_array()
        .into_iter()
        .flatten()
        .map(|a| a.at("PolicyArn").clone())
        .collect();
    out.insert("ManagedPolicyArns".into(), Value::Array(arns));
    Ok(Live::Found(out))
}

async fn function(p: &Probe, name: &str) -> Read {
    let got = p
        .call(
            "lambda",
            "GetFunctionConfiguration",
            json!({"FunctionName": name}),
        )
        .await;
    let Some(c) = found(got)? else {
        return Ok(Live::Gone);
    };
    let mut out = pick(
        &c,
        &[
            ("FunctionName", "FunctionName"),
            ("Runtime", "Runtime"),
            ("Handler", "Handler"),
            ("Role", "Role"),
            ("MemorySize", "MemorySize"),
            ("Timeout", "Timeout"),
            ("Description", "Description"),
            ("Architectures", "Architectures"),
        ],
    );
    let vars = c.at("Environment").at("Variables");
    out.insert(
        "Environment".into(),
        if vars.is_object() {
            json!({"Variables": vars})
        } else {
            Value::Null
        },
    );
    Ok(Live::Found(out))
}

async fn secret(p: &Probe, arn: &str) -> Read {
    let got = p
        .call("secretsmanager", "DescribeSecret", json!({"SecretId": arn}))
        .await;
    let Some(s) = found(got)? else {
        return Ok(Live::Gone);
    };
    if s.get("DeletedDate").is_some_and(|d| !d.is_null()) {
        // Scheduled for deletion: no longer the stack's secret (memory).
        return Ok(Live::Gone);
    }
    let mut out = pick(
        &s,
        &[
            ("Name", "Name"),
            ("Description", "Description"),
            ("KmsKeyId", "KmsKeyId"),
        ],
    );
    out.insert(
        "Tags".into(),
        s.get("Tags").cloned().unwrap_or_else(|| json!([])),
    );
    Ok(Live::Found(out))
}

async fn key(p: &Probe, id: &str) -> Read {
    let got = p.call("kms", "DescribeKey", json!({"KeyId": id})).await;
    let Some(v) = found(got)? else {
        return Ok(Live::Gone);
    };
    let m = v.at("KeyMetadata");
    if m.at("KeyState").as_str() == Some("PendingDeletion") {
        // (memory)
        return Ok(Live::Gone);
    }
    let mut out = pick(
        m,
        &[
            ("Description", "Description"),
            ("Enabled", "Enabled"),
            ("KeyUsage", "KeyUsage"),
            ("KeySpec", "KeySpec"),
            ("MultiRegion", "MultiRegion"),
        ],
    );
    if let Ok(r) = p
        .call("kms", "GetKeyRotationStatus", json!({"KeyId": id}))
        .await
    {
        out.insert(
            "EnableKeyRotation".into(),
            r.at("KeyRotationEnabled").clone(),
        );
    }
    if let Ok(pol) = p
        .call(
            "kms",
            "GetKeyPolicy",
            json!({"KeyId": id, "PolicyName": "default"}),
        )
        .await
    {
        out.insert("KeyPolicy".into(), pol.at("Policy").clone());
    }
    if let Ok(t) = p
        .call("kms", "ListResourceTags", json!({"KeyId": id}))
        .await
    {
        let tags: Vec<Value> = t
            .at("Tags")
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| json!({"Key": t.at("TagKey"), "Value": t.at("TagValue")}))
            .collect();
        out.insert("Tags".into(), Value::Array(tags));
    }
    Ok(Live::Found(out))
}
