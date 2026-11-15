//! `AWS::SSM::Document`, made through SSM's own `CreateDocument`. `Content` may be the document as an object (written as JSON, or as
//! YAML when `DocumentFormat` is `YAML`) or as text. An update writes a new version with `UpdateDocument` and makes it the default
//! (memory: what the handler does with `UpdateMethod: NewVersion`; doze-aws does the same for `Replace`, the default, since a
//! document's content is all that differs). `Ref` is the document's name.

use serde_json::{Value, json};

use super::{Done, Made, Rc, gone, tag_list, text};
use crate::j::J;
use crate::model::Res;

const SVC: &str = "ssm";

fn content(p: &Value) -> String {
    let yaml = text(p, "DocumentFormat").as_deref() == Some("YAML");
    match p.get("Content") {
        Some(Value::String(s)) => s.clone(),
        Some(v) if yaml => aws_yaml::emit(v),
        Some(v) => v.to_string(),
        None => String::new(),
    }
}

pub(super) async fn create(rc: &Rc, p: &Value) -> Done {
    let name = text(p, "Name").unwrap_or_else(|| super::generated_name(rc, 128, false));
    let mut b = json!({"Name": name, "Content": content(p)});
    for k in [
        "DocumentType",
        "DocumentFormat",
        "TargetType",
        "VersionName",
    ] {
        if let Some(v) = text(p, k) {
            b.put(k, v.into());
        }
    }
    if let Some(r) = p.get("Requires").filter(|r| r.is_array()) {
        b.put("Requires", r.clone());
    }
    let tags: Vec<Value> = tag_list(rc, p)
        .into_iter()
        .map(|(k, v)| json!({"Key": k, "Value": v}))
        .collect();
    if !tags.is_empty() {
        b.put("Tags", Value::Array(tags));
    }
    match rc.call(SVC, "CreateDocument", b).await {
        Ok(_) => Ok(Made::new(name)),
        Err(e) if e.code.contains("DocumentAlreadyExists") => Err(rc.taken(&name)),
        Err(e) => Err(rc.fail(SVC, &e)),
    }
}

pub(super) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    let name = old.physical.clone().unwrap_or_default();
    let mut b = json!({"Name": name, "Content": content(p), "DocumentVersion": "$LATEST"});
    for k in ["DocumentFormat", "TargetType", "VersionName"] {
        if let Some(v) = text(p, k) {
            b.put(k, v.into());
        }
    }
    match rc.call(SVC, "UpdateDocument", b).await {
        Ok(out) => {
            let version = out
                .pointer("/DocumentDescription/DocumentVersion")
                .and_then(Value::as_str)
                .unwrap_or("1")
                .to_owned();
            rc.call(
                SVC,
                "UpdateDocumentDefaultVersion",
                json!({"Name": name, "DocumentVersion": version}),
            )
            .await
            .map_err(|e| rc.fail(SVC, &e))?;
        }
        // The content is unchanged (a tag or another property changed): nothing to write as a version.
        Err(e) if e.code.contains("DuplicateDocumentContent") => {}
        Err(e) => return Err(rc.fail(SVC, &e)),
    }
    Ok(Made::new(name))
}

pub(super) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    let name = res.physical.clone().unwrap_or_default();
    match rc.call(SVC, "DeleteDocument", json!({"Name": name})).await {
        Ok(_) => Ok(()),
        Err(e) if gone(&e) || e.code.contains("InvalidDocument") => Ok(()),
        Err(e) => Err(rc.fail(SVC, &e)),
    }
}
