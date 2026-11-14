//! Execution logs of a REST API stage: when the method's settings (its own, else the stage's `*/*`) set `loggingLevel` to `INFO` or
//! `ERROR`, the narrative of each request goes to `API-Gateway-Execution-Logs_<apiId>/<stage>`, every line prefixed with
//! `(<requestId>)`. `INFO` writes every request; `ERROR` only those that ended in an error. `dataTraceEnabled` adds the headers and
//! bodies. HTTP APIs have no execution logs (only access logs), as on AWS.
//!
//! The lines are AWS's as remembered (memory): the order, the wording and the `{key=value, ...}` form of the maps. Only what this
//! request actually went through is written: the integration's endpoint is the method's integration URI, and the answer is the one
//! the client got.

use aws_wire::service::WebReply;
use serde_json::Value;

use super::{Found, Incoming};
use crate::doc::text;

/// How much of a body a data-trace line carries.
const TRACE_BYTES: usize = 10_000;

/// What the method's settings ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Level {
    /// Only requests that ended in an error.
    pub(crate) errors_only: bool,
    pub(crate) data_trace: bool,
}

/// The logging the method's settings ask for (`<path>/<METHOD>`, else `*/*`), or none.
pub(crate) fn level(found: &Found, method: &str) -> Option<Level> {
    let settings = found.stage.get("methodSettings")?;
    let route = found.route.as_ref()?;
    let own = format!("{}/{method}", route.resource_path.trim_start_matches('/'));
    let s = settings.get(&own).or_else(|| settings.get("*/*"))?;
    let errors_only = match text(s, "loggingLevel")? {
        "INFO" => false,
        "ERROR" => true,
        _ => return None,
    };
    Some(Level {
        errors_only,
        data_trace: s.get("dataTraceEnabled").and_then(Value::as_bool) == Some(true),
    })
}

/// A map as AWS's logs show one: `{a=1, b=2}`.
fn map<'a>(pairs: impl Iterator<Item = (&'a str, &'a str)>) -> String {
    let inner: Vec<String> = pairs.map(|(k, v)| format!("{k}={v}")).collect();
    format!("{{{}}}", inner.join(", "))
}

fn trace(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    match text.char_indices().nth(TRACE_BYTES) {
        Some((at, _)) => format!("{} [TRUNCATED]", text.get(..at).unwrap_or_default()),
        None => text.into_owned(),
    }
}

/// The lines of one request, without their `(<requestId>)` prefix.
pub(crate) fn lines(
    found: &Found,
    req: &Incoming,
    answer: &WebReply,
    ms: u128,
    level: Level,
) -> Vec<String> {
    let failed = answer.status >= 400;
    if level.errors_only && !failed {
        return Vec::new();
    }
    let message = serde_json::from_slice::<Value>(&answer.body)
        .ok()
        .and_then(|v| text(&v, "message").map(str::to_owned))
        .unwrap_or_else(|| format!("status {}", answer.status));
    let mut out = Vec::new();
    if !level.errors_only {
        narrative(&mut out, found, req, answer, ms, level.data_trace);
    }
    if answer.status >= 500 {
        out.push(format!(
            "Execution failed due to configuration error: {message}"
        ));
    } else if !level.errors_only {
        out.push("Successfully completed execution".to_owned());
    }
    out.push(format!("Method completed with status: {}", answer.status));
    out
}

/// What `INFO` writes of a request before its outcome.
fn narrative(
    out: &mut Vec<String>,
    found: &Found,
    req: &Incoming,
    answer: &WebReply,
    ms: u128,
    data_trace: bool,
) {
    let id = &req.request_id;
    let stage = format!("{}/{}", found.api_id(), found.stage_name);
    let key = req.header("x-api-key").unwrap_or_default();
    let route = found.route.as_ref();
    let params = route.map(|r| map(r.params.iter().map(|(k, v)| (k.as_str(), v.as_str()))));
    out.push(format!("Extended Request Id: {id}"));
    out.push(format!(
        "Verifying Usage Plan for request: {id}. API Key: {key} API Stage: {stage}"
    ));
    out.push(format!(
        "Usage Plan check succeeded for API Key {key} and API Stage {stage}"
    ));
    out.push(format!("Starting execution for request: {id}"));
    out.push(format!(
        "HTTP Method: {}, Resource Path: {}",
        req.method, found.path
    ));
    out.push(format!(
        "Method request path: {}",
        params.unwrap_or_else(|| "{}".into())
    ));
    out.push(format!(
        "Method request query string: {}",
        map(req.query.iter().map(|(k, v)| (k.as_str(), v.as_str())))
    ));
    if data_trace {
        out.push(format!(
            "Method request headers: {}",
            map(req.headers.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        ));
        out.push(format!(
            "Method request body before transformations: {}",
            trace(&req.body)
        ));
    }
    let uri = route.and_then(|r| text(&r.integration, "uri"));
    if let Some(uri) = uri {
        out.push(format!("Endpoint request URI: {uri}"));
        out.push(format!("Sending request to {uri}"));
    }
    out.push(format!(
        "Received response. Status: {}, Integration latency: {ms} ms",
        answer.status
    ));
    if data_trace {
        out.push(format!(
            "Method response body after transformations: {}",
            trace(&answer.body)
        ));
        out.push(format!(
            "Method response headers: {}",
            map(answer.headers.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        ));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn maps_are_written_as_aws_writes_them() {
        assert_eq!(map([("a", "1"), ("b", "2")].into_iter()), "{a=1, b=2}");
        assert_eq!(map(std::iter::empty()), "{}");
        assert!(trace("x".repeat(TRACE_BYTES + 5).as_bytes()).ends_with(" [TRUNCATED]"));
    }
}
