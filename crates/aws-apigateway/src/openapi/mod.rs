//! OpenAPI import and export: the in-house subset SCOPE.md item 15 asks for. A document (OpenAPI 3.0 or Swagger 2.0, JSON or YAML)
//! becomes a REST API (`ImportRestApi`, `PutRestApi` in `overwrite` or `merge` mode) or an HTTP API (`ImportApi`, `ReimportApi`) with
//! API Gateway's `x-amazon-apigateway-*` extensions read: integrations and their responses, authorizers, request validators,
//! gateway responses, binary media types, the API key source, the policy, CORS. What is not read is reported in `warnings` (and
//! refused with `failOnWarnings`), never silently dropped. Export writes the same document back from a stage.

pub(crate) mod export;
mod rest_paths;
pub(crate) mod v1;
pub(crate) mod v2;
mod yaml;
mod yaml_write;

use aws_wire::ApiError;
use serde_json::Value;

use crate::errors::bad;
use crate::req::Req;

/// The largest document read (API Gateway's own limit for an import is 6 MB).
const MAX_LEN: usize = 6 * 1024 * 1024;

// fuzz:parser fuzz_openapi
/// A document read from JSON or YAML.
///
/// # Errors
/// A document that is too large or cannot be read.
pub(crate) fn read(text: &str) -> Result<Value, String> {
    if text.len() > MAX_LEN {
        return Err("the document is larger than 6 MB".into());
    }
    let t = text.trim_start();
    if t.starts_with('{') {
        return serde_json::from_str(t).map_err(|e| format!("the JSON cannot be read: {e}"));
    }
    yaml::parse(text)
}

/// A document written as YAML.
pub(crate) fn to_yaml(v: &Value) -> String {
    yaml_write::write(v)
}

/// The document a request carries in `body` (a blob, base64 in the request JSON, for v1; a string for v2).
pub(crate) fn body_of(r: &Req, blob: bool) -> Result<Value, ApiError> {
    let raw = r.s("body");
    let text = if blob {
        aws_core::base64::decode(raw)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default()
    } else {
        raw.to_owned()
    };
    let doc = read(&text).map_err(|why| bad(format!("Invalid OpenAPI input: {why}")))?;
    if !doc.is_object() || (doc.get("openapi").is_none() && doc.get("swagger").is_none()) {
        return Err(bad(
            "Invalid OpenAPI input: the document names neither `openapi` nor `swagger`",
        ));
    }
    Ok(doc)
}

/// The methods of an OpenAPI path item and the API Gateway method each is.
pub(crate) fn methods(item: &Value) -> Vec<(&'static str, &Value)> {
    const KEYS: [(&str, &str); 8] = [
        ("get", "GET"),
        ("put", "PUT"),
        ("post", "POST"),
        ("delete", "DELETE"),
        ("patch", "PATCH"),
        ("head", "HEAD"),
        ("options", "OPTIONS"),
        ("x-amazon-apigateway-any-method", "ANY"),
    ];
    KEYS.iter()
        .filter_map(|(k, m)| item.get(*k).map(|v| (*m, v)))
        .collect()
}

/// Upper-cased with `_` kept (`aws_proxy` to `AWS_PROXY`, `when_no_match` to `WHEN_NO_MATCH`).
pub(crate) fn upper(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).map(str::to_ascii_uppercase)
}

/// The security schemes of a document (3.0 `components.securitySchemes` or 2.0 `securityDefinitions`).
pub(crate) fn schemes(doc: &Value) -> Option<&serde_json::Map<String, Value>> {
    doc.pointer("/components/securitySchemes")
        .or_else(|| doc.get("securityDefinitions"))
        .and_then(Value::as_object)
}
