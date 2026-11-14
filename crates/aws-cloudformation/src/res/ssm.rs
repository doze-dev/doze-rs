//! `AWS::SSM::Parameter`.

use serde_json::{Value, json};

use super::{Done, Made, Rc, gone, tag_list, text};
use crate::j::J;
use crate::model::Res;

const SVC: &str = "ssm";

fn body(rc: &Rc, name: &str, p: &Value) -> Value {
    let mut b = json!({"Name": name, "Value": text(p, "Value").unwrap_or_default(),
        "Type": text(p, "Type").unwrap_or_else(|| "String".into())});
    for k in ["Description", "AllowedPattern", "Tier", "DataType"] {
        if let Some(v) = text(p, k) {
            b.put(k, v.into());
        }
    }
    if let Some(v) = super::document(p, "Policies") {
        b.put("Policies", v.into());
    }
    let _ = rc;
    b
}

pub(super) async fn create(rc: &Rc, p: &Value) -> Done {
    if rc.ty != "AWS::SSM::Parameter" {
        return Err(format!("{} is not a type doze-aws makes", rc.ty));
    }
    if text(p, "Type").as_deref() == Some("SecureString") {
        // (memory) CloudFormation does not make SecureString parameters.
        return Err(rc.handler(
            "SecureString is not a supported parameter type for AWS::SSM::Parameter",
            "InvalidRequest",
        ));
    }
    let name = text(p, "Name")
        .unwrap_or_else(|| format!("CFN-{}-{}", rc.logical, crate::model::suffix(12, false)));
    if rc
        .call(SVC, "GetParameter", json!({"Name": name}))
        .await
        .is_ok()
    {
        return Err(rc.taken(&name));
    }
    let mut b = body(rc, &name, p);
    let tags: Vec<Value> = tag_list(rc, p)
        .into_iter()
        .map(|(k, v)| json!({"Key": k, "Value": v}))
        .collect();
    if !tags.is_empty() {
        b.put("Tags", Value::Array(tags));
    }
    rc.call(SVC, "PutParameter", b)
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    Ok(described(&name, p))
}

fn described(name: &str, p: &Value) -> Made {
    Made::new(name)
        .with("Type", text(p, "Type").unwrap_or_else(|| "String".into()))
        .with("Value", text(p, "Value").unwrap_or_default())
}

pub(super) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    let name = old.physical.clone().unwrap_or_default();
    let mut b = body(rc, &name, p);
    b.put("Overwrite", true.into());
    rc.call(SVC, "PutParameter", b)
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    let (then, now) = (tag_list(rc, &old.props_value()), tag_list(rc, p));
    if then != now {
        let removed: Vec<&String> = then
            .iter()
            .map(|(k, _)| k)
            .filter(|k| !now.iter().any(|(n, _)| n == *k))
            .collect();
        if !removed.is_empty() {
            rc.call(
                SVC,
                "RemoveTagsFromResource",
                json!({"ResourceType": "Parameter", "ResourceId": name, "TagKeys": removed}),
            )
            .await
            .map_err(|e| rc.fail(SVC, &e))?;
        }
        if !now.is_empty() {
            let tags: Vec<Value> = now
                .iter()
                .map(|(k, v)| json!({"Key": k, "Value": v}))
                .collect();
            rc.call(
                SVC,
                "AddTagsToResource",
                json!({"ResourceType": "Parameter", "ResourceId": name, "Tags": tags}),
            )
            .await
            .map_err(|e| rc.fail(SVC, &e))?;
        }
    }
    Ok(described(&name, p))
}

pub(super) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    let name = res.physical.clone().unwrap_or_default();
    match rc.call(SVC, "DeleteParameter", json!({"Name": name})).await {
        Ok(_) => Ok(()),
        Err(e) if gone(&e) || e.code.contains("ParameterNotFound") => Ok(()),
        Err(e) => Err(rc.fail(SVC, &e)),
    }
}
