//! Access logs: a stage with `accessLogSettings` writes one line per request, in its format, to its CloudWatch Logs group (over the
//! peer bus, into a stream per stage). The `$context` variables a local request can answer are filled in; any other is `-`, as AWS
//! writes an absent value. Execution logs are not written (SCOPE.md defers them).

use std::time::Instant;

use aws_wire::service::{Caller, WebReply};
use serde_json::{Value, json};

use super::{Found, Incoming};
use crate::Inner;
use crate::doc::text;

/// The value of `$context.<name>` for a request and its answer.
fn variable(
    name: &str,
    found: &Found,
    req: &Incoming,
    answer: &WebReply,
    ms: u128,
) -> Option<String> {
    let route = found.route.as_ref();
    Some(match name {
        "requestId" | "extendedRequestId" => req.request_id.clone(),
        "httpMethod" => req.method.clone(),
        "resourcePath" => route.map(|r| r.resource_path.clone())?,
        "routeKey" => route.map(|r| r.resource_path.clone())?,
        "resourceId" => route.map(|r| r.resource_id.clone())?,
        "path" => format!("/{}{}", found.stage_name, found.path),
        "status" => answer.status.to_string(),
        "responseLength" => answer.body.len().to_string(),
        "responseLatency" | "integrationLatency" | "integration.latency" => ms.to_string(),
        "requestTime" => super::event::clf_time(req.now_ms),
        "requestTimeEpoch" => req.now_ms.to_string(),
        "stage" => found.stage_name.clone(),
        "apiId" | "domainPrefix" => found.api_id().to_owned(),
        "accountId" => found.account.clone(),
        "domainName" => req.host.clone(),
        "protocol" => "HTTP/1.1".into(),
        "identity.sourceIp" => "127.0.0.1".into(),
        "identity.userAgent" => req.header("user-agent")?.to_owned(),
        "error.message" if answer.status >= 400 => serde_json::from_slice::<Value>(&answer.body)
            .ok()
            .and_then(|v| text(&v, "message").map(str::to_owned))?,
        _ => return None,
    })
}

/// The format with every `$context.x` replaced.
fn line(format: &str, found: &Found, req: &Incoming, answer: &WebReply, ms: u128) -> String {
    let mut out = String::with_capacity(format.len() + 64);
    let mut rest = format;
    while let Some(i) = rest.find("$context.") {
        out.push_str(rest.get(..i).unwrap_or_default());
        let after = rest.get(i + "$context.".len()..).unwrap_or_default();
        let end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '_'))
            .unwrap_or(after.len());
        let name = after.get(..end).unwrap_or_default().trim_end_matches('.');
        out.push_str(&variable(name, found, req, answer, ms).unwrap_or_else(|| "-".to_owned()));
        rest = after.get(name.len()..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}

/// Writes the access-log line of a request, if its stage asks for one.
pub(crate) async fn log(
    inner: &Inner,
    found: &Found,
    req: &Incoming,
    answer: &WebReply,
    started: Instant,
) {
    let settings = found.stage.get("accessLogSettings");
    let (Some(arn), Some(format)) = (
        settings.and_then(|s| text(s, "destinationArn")),
        settings.and_then(|s| text(s, "format")),
    ) else {
        return;
    };
    let Some(peers) = inner.peers.as_ref() else {
        return;
    };
    let group = arn
        .split(":log-group:")
        .nth(1)
        .unwrap_or(arn)
        .trim_end_matches(":*");
    let region = arn.split(':').nth(3).unwrap_or(&found.region);
    let stream = format!(
        "{}-{}",
        found.api_id(),
        found.stage_name.trim_start_matches('$')
    );
    let caller = Caller {
        access_key_id: "apigateway".into(),
        account_id: found.account.clone(),
        region: region.to_owned(),
        endpoint: String::new(),
        service: None,
        session_token: None,
    };
    let message = line(format, found, req, answer, started.elapsed().as_millis());
    let names = json!({"logGroupName": group, "logStreamName": stream});
    // WHY: the stream may already exist; that refusal is the expected answer and nothing to act on.
    drop(
        peers
            .call("apigateway", &caller, "logs", "CreateLogStream", &names)
            .await,
    );
    let mut put = names;
    crate::doc::put(
        &mut put,
        "logEvents",
        json!([{"timestamp": req.now_ms, "message": message}]),
    );
    if let Err(e) = peers
        .call("apigateway", &caller, "logs", "PutLogEvents", &put)
        .await
    {
        tracing::warn!(
            "execute-api {}: access log to {group}: {}",
            found.api_id(),
            e.message
        );
    }
}
