//! `AWS::SecretsManager::Secret` (with `GenerateSecretString`, which asks Secrets Manager for a password and puts it in the template
//! object under `GenerateStringKey`, as CloudFormation does) and `AWS::SecretsManager::ResourcePolicy`.

use serde_json::{Value, json};

use super::{Done, Made, Rc, document, gone, tag_list, text};
use crate::j::J;
use crate::model::Res;

const SVC: &str = "secretsmanager";

async fn call(rc: &Rc, op: &str, body: Value) -> Result<Value, String> {
    rc.call(SVC, op, body).await.map_err(|e| rc.fail(SVC, &e))
}

/// The secret's value: as given, or generated.
async fn value(rc: &Rc, p: &Value) -> Result<Option<String>, String> {
    if let Some(s) = text(p, "SecretString") {
        return Ok(Some(s));
    }
    let Some(g) = p.get("GenerateSecretString") else {
        return Ok(None);
    };
    let mut body = json!({});
    for k in [
        "PasswordLength",
        "ExcludeCharacters",
        "ExcludeLowercase",
        "ExcludeNumbers",
        "ExcludePunctuation",
        "ExcludeUppercase",
        "IncludeSpace",
        "RequireEachIncludedType",
    ] {
        if let Some(v) = g.get(k) {
            body.put(k, v.clone());
        }
    }
    let made = call(rc, "GetRandomPassword", body).await?;
    let password = made
        .at("RandomPassword")
        .as_str()
        .unwrap_or_default()
        .to_owned();
    match (
        text(g, "SecretStringTemplate"),
        text(g, "GenerateStringKey"),
    ) {
        (Some(template), Some(key)) => {
            let mut doc: Value = serde_json::from_str(&template).map_err(|_| {
                // (memory)
                rc.handler(
                    "SecretStringTemplate is not a valid JSON object",
                    "InvalidRequest",
                )
            })?;
            doc.put(key, Value::String(password));
            Ok(Some(doc.to_string()))
        }
        _ => Ok(Some(password)),
    }
}

pub(super) async fn create(rc: &Rc, p: &Value) -> Done {
    if rc.ty == "AWS::SecretsManager::ResourcePolicy" {
        let id = text(p, "SecretId").unwrap_or_default();
        put_policy(rc, &id, p).await?;
        return Ok(Made::new(id));
    }
    let name = text(p, "Name")
        .unwrap_or_else(|| format!("{}-{}", rc.logical, crate::model::suffix(12, false)));
    let mut body = json!({"Name": name});
    for k in ["Description", "KmsKeyId"] {
        if let Some(v) = text(p, k) {
            body.put(k, v.into());
        }
    }
    if let Some(v) = value(rc, p).await? {
        body.put("SecretString", v.into());
    }
    let tags: Vec<Value> = tag_list(rc, p)
        .into_iter()
        .map(|(k, v)| json!({"Key": k, "Value": v}))
        .collect();
    if !tags.is_empty() {
        body.put("Tags", Value::Array(tags));
    }
    let made = call(rc, "CreateSecret", body).await?;
    let arn = made.at("ARN").as_str().unwrap_or_default().to_owned();
    Ok(Made::new(&arn).with("Id", arn.clone()))
}

async fn put_policy(rc: &Rc, id: &str, p: &Value) -> Result<(), String> {
    let mut body = json!({"SecretId": id, "ResourcePolicy": document(p, "ResourcePolicy")});
    if let Some(b) = p.get("BlockPublicPolicy") {
        body.put("BlockPublicPolicy", b.clone());
    }
    call(rc, "PutResourcePolicy", body).await.map(|_| ())
}

pub(super) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    let id = old.physical.clone().unwrap_or_default();
    let before = old.props_value();
    if rc.ty == "AWS::SecretsManager::ResourcePolicy" {
        put_policy(rc, &id, p).await?;
        return Ok(Made::new(id));
    }
    let mut body = json!({"SecretId": id});
    let mut changed = false;
    for k in ["Description", "KmsKeyId"] {
        if text(&before, k) != text(p, k) {
            body.put(k, text(p, k).unwrap_or_default().into());
            changed = true;
        }
    }
    // A generated value is generated again only when its recipe changes (as on AWS).
    if (before.get("SecretString") != p.get("SecretString")
        || before.get("GenerateSecretString") != p.get("GenerateSecretString"))
        && let Some(v) = value(rc, p).await?
    {
        body.put("SecretString", v.into());
        changed = true;
    }
    if changed {
        call(rc, "UpdateSecret", body).await?;
    }
    let (then, now) = (tag_list(rc, &before), tag_list(rc, p));
    if then != now {
        let removed: Vec<&String> = then
            .iter()
            .map(|(k, _)| k)
            .filter(|k| !now.iter().any(|(n, _)| n == *k))
            .collect();
        if !removed.is_empty() {
            call(
                rc,
                "UntagResource",
                json!({"SecretId": id, "TagKeys": removed}),
            )
            .await?;
        }
        let tags: Vec<Value> = now
            .iter()
            .map(|(k, v)| json!({"Key": k, "Value": v}))
            .collect();
        if !tags.is_empty() {
            call(rc, "TagResource", json!({"SecretId": id, "Tags": tags})).await?;
        }
    }
    Ok(Made::new(&id).with("Id", id.clone()))
}

pub(super) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    let id = res.physical.clone().unwrap_or_default();
    let (op, body) = if rc.ty == "AWS::SecretsManager::ResourcePolicy" {
        ("DeleteResourcePolicy", json!({"SecretId": id}))
    } else {
        // (memory) CloudFormation deletes a secret without a recovery window, so its name is free again at once.
        (
            "DeleteSecret",
            json!({"SecretId": id, "ForceDeleteWithoutRecovery": true}),
        )
    };
    match rc.call(SVC, op, body).await {
        Ok(_) => Ok(()),
        Err(e) if gone(&e) => Ok(()),
        Err(e) => Err(rc.fail(SVC, &e)),
    }
}
