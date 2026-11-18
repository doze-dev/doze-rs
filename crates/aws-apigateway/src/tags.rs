//! Tags, for both APIs: an ARN `arn:aws:apigateway:<region>::<path>` names the record whose `tags` member is read or changed (a REST or
//! HTTP API, a stage of one, an API key, a usage plan, a domain name).

use aws_wire::ApiError;
use serde_json::{Map, Value};

use crate::core::Core;
use crate::doc::{Doc, child};
use crate::errors::{bad, not_found};

/// The object the ARN names, to change.
fn slot<'a>(core: &'a mut Core, arn: &str) -> Result<&'a mut Value, ApiError> {
    let path = arn
        .split_once("::")
        .map(|(_, p)| p)
        .filter(|_| arn.starts_with("arn:aws:apigateway:"))
        .ok_or_else(|| bad(format!("Invalid ARN specified in the request: {arn}")))?;
    let segs: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let missing = || not_found("Invalid resource identifier specified");
    let doc = match segs.as_slice() {
        ["restapis", id, ..] => core.rest.get_mut(id),
        ["apis", id, ..] => core.http.get_mut(id),
        ["apikeys", id] => core.keys.get_mut(id),
        ["usageplans", id] => core.plans.get_mut(id),
        ["domainnames", name] => core.domains.get_mut(name),
        _ => None,
    };
    let Doc(doc) = doc.ok_or_else(missing)?;
    match segs.as_slice() {
        ["restapis" | "apis", _] => doc.get_mut("api").ok_or_else(missing),
        ["restapis" | "apis", _, "stages", name] => doc
            .get_mut("stages")
            .and_then(|s| s.get_mut(*name))
            .ok_or_else(missing),
        ["apikeys" | "usageplans" | "domainnames", _] => Ok(doc),
        _ => Err(missing()),
    }
}

/// The tags of the record the ARN names.
///
/// # Errors
/// An ARN that names nothing.
pub(crate) fn get(core: &mut Core, arn: &str) -> Result<Value, ApiError> {
    let v = slot(core, arn)?;
    Ok(v.get("tags")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new())))
}

/// Adds (or replaces) tags.
///
/// # Errors
/// An ARN that names nothing.
pub(crate) fn tag(core: &mut Core, arn: &str, tags: Option<&Value>) -> Result<(), ApiError> {
    let v = slot(core, arn)?;
    if let (Some(add), Some(have)) = (tags.and_then(Value::as_object), child(v, "tags")) {
        for (k, x) in add {
            have.insert(k.clone(), x.clone());
        }
    }
    Ok(())
}

/// Removes tags by key.
///
/// # Errors
/// An ARN that names nothing.
pub(crate) fn untag(core: &mut Core, arn: &str, keys: Option<&Value>) -> Result<(), ApiError> {
    let v = slot(core, arn)?;
    if let (Some(keys), Some(have)) = (keys.and_then(Value::as_array), child(v, "tags")) {
        for k in keys.iter().filter_map(Value::as_str) {
            have.remove(k);
        }
    }
    Ok(())
}
