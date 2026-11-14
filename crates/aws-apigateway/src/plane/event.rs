//! The events a Lambda proxy integration receives: payload format 1.0 (REST APIs, and HTTP APIs that ask for it) and 2.0 (HTTP APIs),
//! with the `requestContext` AWS sends. The members a recording checked (`path`, `httpMethod`, `queryStringParameters`, `pathParameters`,
//! `body`, `requestContext.stage`; `rawPath`, `requestContext.http.method`) are recorded; the rest follow AWS's documented event
//! `(memory)` for absent-versus-null details.

use serde_json::{Map, Value, json};

use super::mapping::{canonical, header_map};
use super::route::params_map;
use super::{Found, Incoming, Route};

/// `13/Nov/2026:10:00:00 +0000`, as `requestTime` is written.
pub(crate) fn clf_time(ms: i64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let iso = aws_core::time::format_iso8601(ms / 1000);
    let part = |r: std::ops::Range<usize>| iso.get(r).unwrap_or_default().to_owned();
    let month = part(5..7).parse::<usize>().unwrap_or(1);
    let name = MONTHS
        .get(month.saturating_sub(1))
        .copied()
        .unwrap_or("Jan");
    format!(
        "{}/{name}/{}:{} +0000",
        part(8..10),
        part(0..4),
        part(11..19)
    )
}

/// Whether a body is sent base64: it is not UTF-8, or (REST) its content type is one of the API's binary media types.
pub(crate) fn binary(req: &Incoming, found: &Found) -> bool {
    if std::str::from_utf8(&req.body).is_err() {
        return true;
    }
    let ct = req.header("content-type").unwrap_or_default();
    found
        .api
        .get("binaryMediaTypes")
        .and_then(Value::as_array)
        .is_some_and(|types| {
            types
                .iter()
                .filter_map(Value::as_str)
                .any(|t| media_matches(t, ct))
        })
}

/// Whether media type `pattern` (`image/png`, `image/*`, `*/*`) covers `ct`.
pub(crate) fn media_matches(pattern: &str, ct: &str) -> bool {
    let ct = ct
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let pattern = pattern.to_ascii_lowercase();
    match pattern.split_once('/') {
        Some(("*", "*")) => !ct.is_empty(),
        Some((top, "*")) => ct.split('/').next() == Some(top),
        _ => pattern == ct,
    }
}

fn body_of(req: &Incoming, base64: bool) -> Value {
    if req.body.is_empty() {
        return Value::Null;
    }
    if base64 {
        return Value::String(aws_core::base64::encode(&req.body));
    }
    Value::String(String::from_utf8_lossy(&req.body).into_owned())
}

fn or_null(m: Map<String, Value>) -> Value {
    if m.is_empty() {
        Value::Null
    } else {
        Value::Object(m)
    }
}

fn multi_query(req: &Incoming) -> Map<String, Value> {
    let mut m: Map<String, Value> = Map::new();
    for (k, v) in &req.query {
        let entry = m.entry(k.clone()).or_insert_with(|| json!([]));
        if let Some(list) = entry.as_array_mut() {
            list.push(Value::String(v.clone()));
        }
    }
    m
}

fn multi_headers(req: &Incoming) -> Map<String, Value> {
    let mut m: Map<String, Value> = Map::new();
    for (k, v) in &req.headers {
        let entry = m.entry(canonical(k)).or_insert_with(|| json!([]));
        if let Some(list) = entry.as_array_mut() {
            list.push(Value::String(v.clone()));
        }
    }
    m
}

fn identity(req: &Incoming, extra: &Map<String, Value>) -> Value {
    let mut id = json!({
        "cognitoIdentityPoolId": null, "accountId": null, "cognitoIdentityId": null, "caller": null,
        "sourceIp": "127.0.0.1", "principalOrgId": null, "accessKey": null, "cognitoAuthenticationType": null,
        "cognitoAuthenticationProvider": null, "userArn": null, "userAgent": req.header("user-agent"), "user": null,
    });
    if let Some(o) = id.as_object_mut() {
        o.extend(extra.clone());
    }
    id
}

/// The REST `requestContext` (also `$context` in templates, with the error and authorizer added by the caller).
pub(crate) fn rest_context(found: &Found, route: &Route, req: &Incoming, extra: &Context) -> Value {
    let api = found.api_id();
    let mut ctx = json!({
        "resourceId": route.resource_id,
        "resourcePath": route.resource_path,
        "httpMethod": req.method,
        "extendedRequestId": req.request_id,
        "requestTime": clf_time(req.now_ms),
        "path": format!("/{}{}", found.stage_name, found.path),
        "accountId": found.account,
        "protocol": "HTTP/1.1",
        "stage": found.stage_name,
        "domainPrefix": api,
        "requestTimeEpoch": req.now_ms,
        "requestId": req.request_id,
        "identity": identity(req, &extra.identity),
        "domainName": req.host,
        "deploymentId": crate::doc::text(&found.stage, "deploymentId"),
        "apiId": api,
    });
    if let Some(a) = &extra.authorizer {
        crate::doc::put(&mut ctx, "authorizer", a.clone());
    }
    ctx
}

/// What authorization and API keys add to an event.
#[derive(Debug, Default, Clone)]
pub(crate) struct Context {
    pub(crate) authorizer: Option<Value>,
    pub(crate) identity: Map<String, Value>,
    /// The key a Lambda authorizer named for usage plans (`usageIdentifierKey`), for an API whose key source is `AUTHORIZER`.
    pub(crate) usage_key: Option<String>,
}

/// The payload-format 1.0 event (REST APIs; HTTP APIs with `PayloadFormatVersion: 1.0` add `version` and `routeKey`).
pub(crate) fn v1(found: &Found, route: &Route, req: &Incoming, extra: &Context) -> Value {
    let base64 = binary(req, found);
    let mut ctx = rest_context(found, route, req, extra);
    let mut e = json!({
        "resource": route.resource_path,
        "path": found.path,
        "httpMethod": req.method,
        "headers": or_null(header_map(req)),
        "multiValueHeaders": or_null(multi_headers(req)),
        "queryStringParameters": or_null(req.query.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect()),
        "multiValueQueryStringParameters": or_null(multi_query(req)),
        "pathParameters": or_null(params_map(&route.params)),
        "stageVariables": found.variables().map_or(Value::Null, |m| Value::Object(m.clone())),
        "body": body_of(req, base64),
        "isBase64Encoded": base64,
    });
    if found.kind != super::Kind::Rest {
        crate::doc::put(&mut e, "version", json!("1.0"));
        crate::doc::put(
            &mut e,
            "resource",
            json!(
                route
                    .resource_path
                    .split_once(' ')
                    .map_or("$default", |(_, p)| p)
            ),
        );
        crate::doc::put(&mut ctx, "routeKey", json!(route.resource_path));
        crate::doc::put(
            &mut ctx,
            "resourcePath",
            json!(
                route
                    .resource_path
                    .split_once(' ')
                    .map_or("$default", |(_, p)| p)
            ),
        );
    }
    crate::doc::put(&mut e, "requestContext", ctx);
    e
}

/// The path an HTTP API's event shows: with the stage in front, unless the stage is `$default`.
pub(crate) fn raw_path(found: &Found) -> String {
    if found.stage_name == "$default" {
        found.path.clone()
    } else {
        format!("/{}{}", found.stage_name, found.path)
    }
}

/// The HTTP API `requestContext` of payload format 2.0.
pub(crate) fn v2_context(found: &Found, route: &Route, req: &Incoming, extra: &Context) -> Value {
    let api = found.api_id();
    let mut ctx = json!({
        "accountId": found.account,
        "apiId": api,
        "domainName": req.host,
        "domainPrefix": api,
        "http": {
            "method": req.method, "path": raw_path(found), "protocol": "HTTP/1.1",
            "sourceIp": "127.0.0.1", "userAgent": req.header("user-agent").unwrap_or_default(),
        },
        "requestId": req.request_id,
        "routeKey": route.resource_path,
        "stage": found.stage_name,
        "time": clf_time(req.now_ms),
        "timeEpoch": req.now_ms,
    });
    if let Some(a) = &extra.authorizer {
        crate::doc::put(&mut ctx, "authorizer", a.clone());
    }
    ctx
}

/// The payload-format 2.0 event.
pub(crate) fn v2(found: &Found, route: &Route, req: &Incoming, extra: &Context) -> Value {
    let base64 = binary(req, found);
    let mut e = Map::new();
    e.insert("version".into(), json!("2.0"));
    e.insert("routeKey".into(), json!(route.resource_path));
    e.insert("rawPath".into(), json!(raw_path(found)));
    e.insert("rawQueryString".into(), json!(req.raw_query));
    let cookies: Vec<Value> = req
        .headers
        .iter()
        .filter(|(k, _)| k == "cookie")
        .flat_map(|(_, v)| {
            v.split(';')
                .map(|c| Value::String(c.trim().to_owned()))
                .collect::<Vec<_>>()
        })
        .collect();
    if !cookies.is_empty() {
        e.insert("cookies".into(), Value::Array(cookies));
    }
    let mut headers: Map<String, Value> = Map::new();
    for (k, v) in req.headers.iter().filter(|(k, _)| k != "cookie") {
        let joined = match headers.get(k).and_then(Value::as_str) {
            Some(prev) => format!("{prev},{v}"),
            None => v.clone(),
        };
        headers.insert(k.clone(), Value::String(joined));
    }
    e.insert("headers".into(), Value::Object(headers));
    let mut query: Map<String, Value> = Map::new();
    for (k, v) in &req.query {
        let joined = match query.get(k).and_then(Value::as_str) {
            Some(prev) => format!("{prev},{v}"),
            None => v.clone(),
        };
        query.insert(k.clone(), Value::String(joined));
    }
    if !query.is_empty() {
        e.insert("queryStringParameters".into(), Value::Object(query));
    }
    if !route.params.is_empty() {
        e.insert(
            "pathParameters".into(),
            Value::Object(params_map(&route.params)),
        );
    }
    if let Some(vars) = found.variables() {
        e.insert("stageVariables".into(), Value::Object(vars.clone()));
    }
    e.insert(
        "requestContext".into(),
        v2_context(found, route, req, extra),
    );
    if !req.body.is_empty() {
        e.insert("body".into(), body_of(req, base64));
    }
    e.insert("isBase64Encoded".into(), json!(base64));
    Value::Object(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_time_is_written_as_api_gateway_writes_it() {
        assert_eq!(clf_time(1_763_028_000_000), "13/Nov/2025:10:00:00 +0000");
    }

    #[test]
    fn media_types_match_with_wildcards() {
        assert!(media_matches("image/*", "image/png"));
        assert!(media_matches("*/*", "application/octet-stream"));
        assert!(media_matches(
            "application/pdf",
            "application/pdf; charset=x"
        ));
        assert!(!media_matches("image/*", "text/plain"));
    }
}
