//! The answers the plane writes: a function's or a backend's, and API Gateway's own (gateway responses), which a REST API may
//! customise per type. The default body is `{"message":"..."}` (recorded: `Missing Authentication Token`, 403).

use aws_wire::service::WebReply;
use bytes::Bytes;
use serde_json::{Value, json};

use super::{Found, Incoming, vtl};
use crate::gwresp;

/// `{"message": msg}` with `status`.
pub(crate) fn message(status: u16, msg: &str) -> WebReply {
    WebReply {
        status,
        headers: vec![("Content-Type".into(), "application/json".into())],
        body: Bytes::from(json!({"message": msg}).to_string()),
    }
}

/// An answer as it is.
pub(crate) fn of(status: u16, headers: Vec<(String, String)>, body: Bytes) -> WebReply {
    WebReply {
        status,
        headers,
        body,
    }
}

/// The `x-amzn-ErrorType` AWS sends with some of its own answers `(memory)`.
fn error_type(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "MISSING_AUTHENTICATION_TOKEN" => "MissingAuthenticationTokenException",
        "ACCESS_DENIED" => "AccessDeniedException",
        "UNAUTHORIZED" => "UnauthorizedException",
        "INVALID_API_KEY" => "ForbiddenException",
        "BAD_REQUEST_BODY" | "BAD_REQUEST_PARAMETERS" => "BadRequestException",
        _ => return None,
    })
}

/// API Gateway's own answer of type `kind` (`MISSING_AUTHENTICATION_TOKEN`, `UNAUTHORIZED`...) for a REST API: the API's own
/// response for the type, else its `DEFAULT_4XX`/`DEFAULT_5XX`, else AWS's default; the template run with `$context.error`.
pub(crate) fn gateway(
    found: &Found,
    kind: &str,
    status: u16,
    msg: &str,
    req: &Incoming,
    ctx: Option<&Value>,
) -> WebReply {
    let own = Some(&found.responses);
    let resp = own
        .and_then(|m| m.get(kind))
        .or_else(|| own.and_then(|m| m.get(gwresp::fallback(status))))
        .cloned()
        .unwrap_or_else(|| gwresp::default_of(kind));
    let status = crate::doc::text(&resp, "statusCode")
        .and_then(|s| s.parse().ok())
        .unwrap_or(status);
    let mut context = ctx.cloned().unwrap_or_else(|| json!({}));
    crate::doc::put(
        &mut context,
        "error",
        // AWS words a failed authorizer's answer as `{"message":null}`: a message of `null` is written bare.
        json!({"message": msg, "messageString": if msg == "null" { "null".to_owned() } else { Value::String(msg.to_owned()).to_string() }, "responseType": kind}),
    );
    let templates = crate::doc::obj(&resp, "responseTemplates");
    let template = templates
        .and_then(|t| t.get("application/json").or_else(|| t.values().next()))
        .and_then(Value::as_str);
    let body = template.map_or_else(
        || json!({"message": msg}).to_string(),
        |t| {
            let mut env = vtl::Env::default();
            env.vars.insert("context".into(), context.clone());
            vtl::render(t, &mut env).unwrap_or_else(|_| json!({"message": msg}).to_string())
        },
    );
    let mut headers = vec![("Content-Type".to_owned(), "application/json".to_owned())];
    if let Some(t) = error_type(kind) {
        headers.push(("x-amzn-ErrorType".into(), t.into()));
    }
    let vars = found
        .variables()
        .map_or(Value::Null, |m| Value::Object(m.clone()));
    for (k, v) in crate::doc::obj(&resp, "responseParameters")
        .into_iter()
        .flatten()
    {
        let (Some(name), Some(src)) = (k.strip_prefix("gatewayresponse.header."), v.as_str())
        else {
            continue;
        };
        if let Some(value) = super::mapping::rest_source(src, req, &[], &context, &vars) {
            headers.retain(|(h, _)| !h.eq_ignore_ascii_case(name));
            headers.push((name.to_owned(), value));
        }
    }
    of(status, headers, Bytes::from(body))
}
