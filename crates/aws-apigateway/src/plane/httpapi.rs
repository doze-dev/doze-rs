//! An HTTP API serving one request: CORS preflight, the route (404 `Not Found` when none, `$default` taking what nothing else does),
//! the authorizer (`JWT`, `CUSTOM` Lambda, `AWS_IAM`), the integration (Lambda proxy in payload 1.0 or 2.0, HTTP proxy, an AWS
//! service by subtype), CORS headers, and the access log.

use aws_wire::service::WebReply;
use bytes::Bytes;
use serde_json::Value;

use super::client::{self, Out};
use super::event::{self, Context};
use super::mapping::v2_source;
use super::{Found, Incoming, Route, auth, cors, jwt, lambda, reply};
use crate::Inner;
use crate::doc::text;

pub(crate) async fn run(inner: &Inner, found: Found, req: Incoming) -> WebReply {
    let started = std::time::Instant::now();
    let config = found
        .api
        .get("corsConfiguration")
        .filter(|c| c.is_object())
        .cloned();
    if let Some(pre) = cors::preflight(config.as_ref(), &req) {
        return pre;
    }
    let mut answer = serve(inner, &found, &req).await;
    cors::decorate(config.as_ref(), &req, &mut answer);
    super::access::log(inner, &found, &req, &answer, started).await;
    answer
}

async fn serve(inner: &Inner, found: &Found, req: &Incoming) -> WebReply {
    let Some(route) = found.route.as_ref() else {
        return reply::message(404, "Not Found");
    };
    let mut extra = Context::default();
    let arn = super::rest::method_arn(found, &req.method);
    let authorized = match text(&route.method, "authorizationType").unwrap_or("NONE") {
        "JWT" => jwt::v2(&inner.plane.jwks, found, route, req, &mut extra).await,
        "CUSTOM" => auth::lambda_v2(inner, found, route, req, &mut extra).await,
        "AWS_IAM" => auth::iam(inner, found, req, &arn, &mut extra).await,
        _ => Ok(()),
    };
    if let Err(denied) = authorized {
        return denied;
    }
    let integration = &route.integration;
    match text(integration, "integrationType").unwrap_or_default() {
        "AWS_PROXY" if text(integration, "integrationSubtype").is_some() => {
            let ctx = event::v2_context(found, route, req, &extra);
            super::aws::v2(inner, found, route, req, &ctx).await
        }
        "AWS_PROXY" => proxy(inner, found, route, req, &extra).await,
        "HTTP_PROXY" => http(found, route, req, &extra).await,
        "" => reply::message(500, "Internal Server Error"),
        other => reply::message(
            500,
            &format!("The integration type {other} cannot serve an HTTP API"),
        ),
    }
}

async fn proxy(
    inner: &Inner,
    found: &Found,
    route: &Route,
    req: &Incoming,
    extra: &Context,
) -> WebReply {
    let integration = &route.integration;
    let uri = text(integration, "integrationUri").unwrap_or_default();
    let Some(function) = lambda::function_of(uri, found.variables()) else {
        return reply::message(500, "Internal Server Error");
    };
    let v2 = text(integration, "payloadFormatVersion") != Some("1.0");
    let ev = if v2 {
        event::v2(found, route, req, extra)
    } else {
        event::v1(found, route, req, extra)
    };
    let source = super::rest::method_arn(found, &req.method);
    let out = match lambda::invoke(
        inner,
        &found.account,
        &function,
        &source,
        ev.to_string().as_bytes(),
    )
    .await
    {
        Ok(o) if o.error.is_none() => o,
        Ok(_) => return reply::message(500, "Internal Server Error"),
        Err(why) => {
            tracing::warn!(
                "execute-api {}: the function could not be invoked: {why}",
                found.api_id()
            );
            return reply::message(500, "Internal Server Error");
        }
    };
    let decoded = if v2 {
        lambda::proxy_v2(&out.payload)
    } else {
        lambda::proxy_v1(&out.payload)
    };
    match decoded {
        Ok(a) => reply::of(a.status, a.headers, a.body),
        Err(why) => {
            tracing::warn!(
                "execute-api {}: malformed Lambda proxy response: {why}",
                found.api_id()
            );
            reply::message(500, "Internal Server Error")
        }
    }
}

/// An `HTTP_PROXY` integration: the request passed to the URL (its `{variables}` filled from the route's), with the integration's
/// parameter mappings (`overwrite:path`, `append:`/`overwrite:`/`remove:` on `header.x` and `querystring.x`).
async fn http(found: &Found, route: &Route, req: &Incoming, extra: &Context) -> WebReply {
    let out = request_of(found, route, req, extra);
    match client::call(&out).await {
        Ok(a) => reply::of(a.status, a.headers, a.body),
        Err(aws_webhook::Failure::Timeout(_)) => reply::message(504, "Gateway Timeout"),
        Err(why) => {
            tracing::warn!(
                "execute-api {}: HTTP integration failed: {why}",
                found.api_id()
            );
            reply::of(
                500,
                vec![("Content-Type".into(), "application/json".into())],
                Bytes::from_static(b"{\"message\":\"Internal Server Error\"}"),
            )
        }
    }
}

/// The request an `HTTP_PROXY` integration sends.
fn request_of(found: &Found, route: &Route, req: &Incoming, extra: &Context) -> Out {
    let integration = &route.integration;
    let mut url = super::lambda::fill_vars(
        text(integration, "integrationUri").unwrap_or_default(),
        found.variables(),
    );
    for (k, v) in &route.params {
        url = url
            .replace(&format!("{{{k}}}"), v)
            .replace(&format!("{{{k}+}}"), v);
    }
    let mut headers: Vec<(String, String)> = req
        .headers
        .iter()
        .filter(|(k, _)| {
            !matches!(
                k.as_str(),
                "host" | "connection" | "content-length" | "transfer-encoding"
            )
        })
        .cloned()
        .collect();
    let mut query = req.query.clone();
    let ctx = event::v2_context(found, route, req, extra);
    let vars = found
        .variables()
        .map_or(Value::Null, |m| Value::Object(m.clone()));
    let raw = event::raw_path(found);
    for (k, v) in crate::doc::obj(integration, "requestParameters")
        .into_iter()
        .flatten()
    {
        let value = v2_source(
            v.as_str().unwrap_or_default(),
            req,
            &route.params,
            &raw,
            &ctx,
            &vars,
        );
        let Some((action, target)) = k.split_once(':') else {
            continue;
        };
        apply_mapping(action, target, value, &mut url, &mut headers, &mut query);
    }
    super::mapping::with_query(&mut url, &query);
    let method = match text(integration, "integrationMethod") {
        Some(m) if m != "ANY" => m.to_owned(),
        _ => req.method.clone(),
    };
    let timeout = integration
        .get("timeoutInMillis")
        .and_then(Value::as_u64)
        .unwrap_or(30_000);
    Out {
        headers,
        body: req.body.clone(),
        ..Out::new(method, url, std::time::Duration::from_millis(timeout))
    }
}

fn apply_mapping(
    action: &str,
    target: &str,
    value: String,
    url: &mut String,
    headers: &mut Vec<(String, String)>,
    query: &mut Vec<(String, String)>,
) {
    if target == "path" && action == "overwrite" {
        let base_end = url.find("://").map_or(0, |i| i + 3);
        let path_at = url
            .get(base_end..)
            .and_then(|r| r.find('/'))
            .map_or(url.len(), |i| base_end + i);
        url.truncate(path_at);
        url.push_str(&value);
        return;
    }
    let (list, name) = if let Some(h) = target.strip_prefix("header.") {
        (&mut *headers, h.to_ascii_lowercase())
    } else if let Some(q) = target.strip_prefix("querystring.") {
        (&mut *query, q.to_owned())
    } else {
        return;
    };
    match action {
        "append" => list.push((name, value)),
        "overwrite" => {
            list.retain(|(k, _)| !k.eq_ignore_ascii_case(&name));
            list.push((name, value));
        }
        "remove" => list.retain(|(k, _)| !k.eq_ignore_ascii_case(&name)),
        _ => {}
    }
}
