//! Non-proxy integrations of a REST API (Lambda custom, AWS service, HTTP, MOCK): the request template picked by content type (or the
//! body passed through, as `passthroughBehavior` says), the backend called, the integration response selected by its
//! `selectionPattern` (against the status code for HTTP and MOCK, the error message for Lambda), and the response template and header
//! mappings applied.

use aws_wire::service::WebReply;
use bytes::Bytes;
use serde_json::{Map, Value};

use super::mapping::rest_source;
use super::reply;
use super::{Found, Incoming, Route, vtl};

/// What the request side produced: the body for the backend, and the template's `$context` (for `requestOverride`).
pub(crate) struct Mapped {
    pub(crate) body: String,
    pub(crate) context: Value,
}

/// A backend's answer, before the integration response is applied.
pub(crate) struct Backend {
    /// What selection patterns are matched against.
    pub(crate) selector: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: String,
}

/// A template environment for this request.
pub(crate) fn env(
    found: &Found,
    route: &Route,
    req: &Incoming,
    context: &Value,
    body: String,
) -> vtl::Env {
    let mut env = vtl::Env {
        input: vtl::Input {
            body,
            params: super::mapping::params_of(req, &route.params),
        },
        ..vtl::Env::default()
    };
    env.vars.insert("context".into(), context.clone());
    let vars = found
        .variables()
        .map_or(Value::Object(Map::new()), |m| Value::Object(m.clone()));
    env.vars.insert("stageVariables".into(), vars);
    env
}

fn content_type(req: &Incoming) -> String {
    req.header("content-type")
        .and_then(|c| c.split(';').next())
        .map_or("application/json".to_owned(), |c| {
            c.trim().to_ascii_lowercase()
        })
}

/// The request template's output, or the refusal: 415 when no template fits and the integration does not pass bodies through.
pub(crate) fn request(
    found: &Found,
    route: &Route,
    req: &Incoming,
    context: &Value,
) -> Result<Mapped, WebReply> {
    let integration = &route.integration;
    let templates = crate::doc::obj(integration, "requestTemplates");
    let ct = content_type(req);
    let raw = String::from_utf8_lossy(&req.body).into_owned();
    let behaviour = crate::doc::text(integration, "passthroughBehavior").unwrap_or("WHEN_NO_MATCH");
    let Some(template) = templates.and_then(|t| t.get(&ct)).and_then(Value::as_str) else {
        let none_defined = templates.is_none_or(Map::is_empty);
        return match behaviour {
            "WHEN_NO_MATCH" => Ok(Mapped {
                body: raw,
                context: context.clone(),
            }),
            "WHEN_NO_TEMPLATES" if none_defined => Ok(Mapped {
                body: raw,
                context: context.clone(),
            }),
            _ => Err(reply::gateway(
                found,
                "UNSUPPORTED_MEDIA_TYPE",
                415,
                "Unsupported Media Type",
                req,
                Some(context),
            )),
        };
    };
    let mut e = env(found, route, req, context, raw);
    match vtl::render(template, &mut e) {
        Ok(body) => Ok(Mapped {
            body,
            context: e.vars.remove("context").unwrap_or(Value::Null),
        }),
        Err(why) => Err(config_error(found, req, context, &why.0)),
    }
}

/// An integration that cannot run as configured: 500 `Internal server error`, the reason in doze-aws's log.
pub(crate) fn config_error(found: &Found, req: &Incoming, context: &Value, why: &str) -> WebReply {
    tracing::warn!(
        "execute-api {}: Execution failed due to configuration error: {why}",
        found.api_id()
    );
    reply::gateway(
        found,
        "API_CONFIGURATION_ERROR",
        500,
        "Internal server error",
        req,
        Some(context),
    )
}

/// Whether `pattern` matches the whole of `s` (selection patterns are Java regular expressions matched in full).
fn full_match(pattern: &str, s: &str) -> bool {
    regex_lite::Regex::new(&format!("^(?s:{pattern})$")).is_ok_and(|r| r.is_match(s))
}

/// The integration response whose selection pattern matches, else the default one (no pattern).
fn select<'a>(route: &'a Route, selector: &str) -> Option<&'a Value> {
    let rs = crate::doc::obj(&route.integration, "integrationResponses")?;
    let pattern = |r: &Value| {
        crate::doc::text(r, "selectionPattern")
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
    };
    rs.values()
        .find(|r| pattern(r).is_some_and(|p| full_match(&p, selector)))
        .or_else(|| rs.values().find(|r| pattern(r).is_none()))
}

/// The integration response for a backend's answer, then its template and header mappings.
pub(crate) fn respond(
    found: &Found,
    route: &Route,
    req: &Incoming,
    context: &Value,
    backend: Backend,
) -> WebReply {
    let Some(resp) = select(route, &backend.selector) else {
        return config_error(
            found,
            req,
            context,
            "No match for output mapping and no default output mapping specified",
        );
    };
    let status: u16 = crate::doc::text(resp, "statusCode")
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let templates = crate::doc::obj(resp, "responseTemplates");
    let accept = req
        .header("accept")
        .and_then(|a| a.split(',').next())
        .map(str::trim)
        .unwrap_or("application/json");
    let chosen = templates.and_then(|t| {
        t.get_key_value(accept)
            .or_else(|| t.get_key_value("application/json"))
            .or_else(|| t.iter().next())
    });
    let mut ct = backend
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map_or("application/json".to_owned(), |(_, v)| v.clone());
    let mut ctx = context.clone();
    let body = match chosen {
        Some((key, Value::String(t))) if !t.is_empty() => {
            ct.clone_from(key);
            let mut e = env(found, route, req, context, backend.body.clone());
            match vtl::render(t, &mut e) {
                Ok(b) => {
                    ctx = e.vars.remove("context").unwrap_or(ctx);
                    b
                }
                Err(why) => return config_error(found, req, context, &why.0),
            }
        }
        _ => backend.body.clone(),
    };
    let mut headers = vec![("Content-Type".to_owned(), ct)];
    response_headers(resp, &backend, req, &ctx, found, &mut headers);
    let status = ctx
        .pointer("/responseOverride/status")
        .and_then(Value::as_u64)
        .and_then(|s| u16::try_from(s).ok())
        .unwrap_or(status);
    for (k, v) in ctx
        .pointer("/responseOverride/header")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        headers.retain(|(h, _)| !h.eq_ignore_ascii_case(k));
        headers.push((k.clone(), super::mapping::plain(v)));
    }
    reply::of(status, headers, Bytes::from(body))
}

/// `method.response.header.X` mappings from `integration.response.header.Y`, `integration.response.body[.path]` and the usual sources.
fn response_headers(
    resp: &Value,
    backend: &Backend,
    req: &Incoming,
    ctx: &Value,
    found: &Found,
    headers: &mut Vec<(String, String)>,
) {
    let vars = found
        .variables()
        .map_or(Value::Null, |m| Value::Object(m.clone()));
    for (k, v) in crate::doc::obj(resp, "responseParameters")
        .into_iter()
        .flatten()
    {
        let (Some(name), Some(src)) = (k.strip_prefix("method.response.header."), v.as_str())
        else {
            continue;
        };
        let value = if let Some(h) = src.strip_prefix("integration.response.header.") {
            backend
                .headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(h))
                .map(|(_, v)| v.clone())
        } else if src == "integration.response.body" {
            Some(backend.body.clone())
        } else if let Some(p) = src.strip_prefix("integration.response.body.") {
            serde_json::from_str::<Value>(&backend.body)
                .ok()
                .and_then(|doc| super::mapping::jsonpath(&doc, &format!("$.{p}")))
                .map(|v| super::mapping::plain(&v))
        } else {
            rest_source(src, req, &[], ctx, &vars)
        };
        if let Some(value) = value {
            headers.retain(|(h, _)| !h.eq_ignore_ascii_case(name));
            headers.push((name.to_owned(), value));
        }
    }
}

/// A MOCK integration: the request template's `statusCode` (200 when it says none) selects the response.
pub(crate) fn mock(found: &Found, route: &Route, req: &Incoming, context: &Value) -> WebReply {
    let mapped = match request(found, route, req, context) {
        Ok(m) => m,
        Err(reply) => return reply,
    };
    let status = serde_json::from_str::<Value>(&mapped.body)
        .ok()
        .and_then(|v| {
            v.get("statusCode").and_then(|s| {
                s.as_u64()
                    .or_else(|| s.as_str().and_then(|t| t.parse().ok()))
            })
        })
        .unwrap_or(200);
    let backend = Backend {
        selector: status.to_string(),
        headers: Vec::new(),
        body: String::new(),
    };
    respond(found, route, req, &mapped.context, backend)
}
