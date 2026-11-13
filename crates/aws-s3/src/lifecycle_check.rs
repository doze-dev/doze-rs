//! The checks AWS makes of a lifecycle configuration when it is put. (Wording is from memory: no recording reaches a bad one.)

use aws_wire::ApiError;
use serde_json::Value;

use crate::errors::malformed_xml;

/// The most rules a configuration may hold.
const MAX_RULES: usize = 1000;

fn bad(message: &str) -> ApiError {
    ApiError::new(400, "InvalidArgument", message)
}

/// Checks the rules of `body` (the `LifecycleConfiguration`).
pub(crate) fn rules(body: &Value) -> Result<(), ApiError> {
    let rules = body
        .get("Rules")
        .and_then(Value::as_array)
        .filter(|r| !r.is_empty() && r.len() <= MAX_RULES)
        .ok_or_else(malformed_xml)?;
    let mut ids: Vec<&str> = Vec::new();
    for r in rules {
        if let Some(id) = r.get("ID").and_then(Value::as_str) {
            if id.len() > 255 {
                return Err(bad("ID length should not exceed allowed limit of 255"));
            }
            if ids.contains(&id) {
                return Err(bad(
                    "Rule ID must be unique. Found same ID for more than one rule",
                ));
            }
            ids.push(id);
        }
        rule(r)?;
    }
    Ok(())
}

fn rule(r: &Value) -> Result<(), ApiError> {
    if !matches!(
        r.get("Status").and_then(Value::as_str),
        Some("Enabled" | "Disabled")
    ) {
        return Err(malformed_xml());
    }
    if r.get("Prefix").is_some() && r.get("Filter").is_some() {
        return Err(malformed_xml());
    }
    let has_action = [
        "Expiration",
        "Transitions",
        "NoncurrentVersionExpiration",
        "NoncurrentVersionTransitions",
        "AbortIncompleteMultipartUpload",
    ]
    .iter()
    .any(|a| r.get(*a).is_some());
    if !has_action {
        return Err(malformed_xml());
    }
    if let Some(f) = r.get("Filter") {
        filter(f)?;
    }
    if let Some(e) = r.get("Expiration") {
        let fields = ["Days", "Date", "ExpiredObjectDeleteMarker"]
            .iter()
            .filter(|k| e.get(**k).is_some())
            .count();
        if fields != 1 || e.get("Days").and_then(Value::as_i64).is_some_and(|d| d < 1) {
            return Err(malformed_xml());
        }
    }
    Ok(())
}

/// A filter holds one predicate, or `And` with at least two.
fn filter(f: &Value) -> Result<(), ApiError> {
    let direct = [
        "Prefix",
        "Tag",
        "ObjectSizeGreaterThan",
        "ObjectSizeLessThan",
    ]
    .iter()
    .filter(|k| f.get(**k).is_some())
    .count();
    match (f.get("And"), direct) {
        (None, 0 | 1) => Ok(()),
        (Some(and), 0) => {
            let n = [
                "Prefix",
                "Tags",
                "ObjectSizeGreaterThan",
                "ObjectSizeLessThan",
            ]
            .iter()
            .filter(|k| {
                and.get(**k)
                    .is_some_and(|v| v.as_array().is_none_or(|a| !a.is_empty()))
            })
            .count();
            let tags = and
                .get("Tags")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            if n + tags.saturating_sub(1) >= 2 {
                Ok(())
            } else {
                Err(malformed_xml())
            }
        }
        _ => Err(malformed_xml()),
    }
}
