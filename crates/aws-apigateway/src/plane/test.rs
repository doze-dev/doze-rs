//! `TestInvokeMethod` and `TestInvokeAuthorizer`: the REST API as it is now (not as a stage last deployed it) answering one request
//! made up from the call, through the same code a deployed stage runs. The actor finds the method or authorizer and builds the
//! request ([`find_method`], [`find_authorizer`]); the call is then served outside it ([`run`]), because an integration or an
//! authorizer function can take seconds.
//!
//! As AWS documents (the CLI reference's examples): the answer carries the status, body, headers (single and multi-valued), the
//! latency in milliseconds and the execution log, `Execution log for request <id>` and then one `<date> : <line>` per step,
//! `<date>` as `Wed Feb 24 00:51:47 UTC 2016`; the authorizer test answers `clientStatus` 0 when the authorizer allowed, the
//! principal, the policy as JSON text, and the identity it was given. The lines between the first and the last are the execution
//! log's (`execlog.rs`, `(memory)`). The test stage is `test-invoke-stage` and a test skips authorization and the API key
//! `(memory)`; stage variables are the call's.

use std::time::Instant;

use aws_wire::ApiError;
use aws_wire::service::WebReply;
use bytes::Bytes;
use serde_json::{Map, Value, json};

use super::event::Context;
use super::execlog::{self, Level};
use super::{Found, Incoming, Kind, Route};
use crate::Inner;
use crate::core::Core;
use crate::doc::{obj, put, text};
use crate::errors::{invalid_id, no_api};
use crate::req::{Out, Req, Res};

/// The stage a test runs on `(memory)`.
const STAGE: &str = "test-invoke-stage";

/// A test found by the actor, to be served outside it.
#[derive(Debug)]
pub(crate) enum Pending {
    Method(Box<Found>, Box<Incoming>),
    Authorizer(Box<Found>, Box<Incoming>, Map<String, Value>),
}

/// The request a test describes: `pathWithQueryString`, `headers` and `multiValueHeaders`, `body`.
fn incoming(r: &Req, method: &str, host: String) -> Incoming {
    let raw_query = r
        .s("pathWithQueryString")
        .split_once('?')
        .map(|(_, q)| q.to_owned())
        .unwrap_or_default();
    let mut headers: Vec<(String, String)> = Vec::new();
    for (k, v) in r
        .value("headers")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        headers.push((k.to_ascii_lowercase(), crate::doc::plain(v)));
    }
    for (k, vs) in r
        .value("multiValueHeaders")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let name = k.to_ascii_lowercase();
        headers.retain(|(h, _)| *h != name);
        for v in vs.as_array().into_iter().flatten() {
            headers.push((name.clone(), crate::doc::plain(v)));
        }
    }
    Incoming {
        method: method.to_owned(),
        query: super::mapping::query_pairs(&raw_query),
        raw_query,
        headers,
        body: Bytes::from(r.s("body").to_owned()),
        host,
        request_id: aws_wire::RequestId::new().as_str().to_owned(),
        now_ms: r.now_ms,
    }
}

/// The path a test names (without its query), or `fallback`.
fn path_of(r: &Req, fallback: &str) -> String {
    let p = r.s("pathWithQueryString");
    let p = p.split_once('?').map_or(p, |(p, _)| p);
    if p.is_empty() {
        fallback.to_owned()
    } else {
        format!("/{}", p.trim_start_matches('/'))
    }
}

/// The API as it is now, as a stage named [`STAGE`] with the call's stage variables would serve it.
fn found_of(core: &Core, r: &Req) -> Result<(Found, Value), ApiError> {
    let id = r.s("restApiId");
    let doc = core
        .rest
        .get(id)
        .map(|d| &d.0)
        .ok_or_else(|| no_api(r.account(), id))?;
    let snap = crate::v1::snapshot(doc);
    let mut stage = json!({"stageName": STAGE, "methodSettings": {}});
    if let Some(v) = r.value("stageVariables") {
        put(&mut stage, "variables", v.clone());
    }
    let found = Found {
        kind: Kind::Rest,
        account: text(doc, "account").unwrap_or(r.account()).to_owned(),
        region: text(doc, "region").unwrap_or(r.region()).to_owned(),
        api: snap.get("api").cloned().unwrap_or(Value::Null),
        stage,
        stage_name: STAGE.to_owned(),
        path: "/".to_owned(),
        route: None,
        responses: obj(&snap, "responses").cloned().unwrap_or_default(),
        models: obj(&snap, "models").cloned().unwrap_or_default(),
    };
    Ok((found, snap))
}

fn host(r: &Req) -> String {
    format!(
        "{}.execute-api.{}.amazonaws.com",
        r.s("restApiId"),
        r.region()
    )
}

/// `TestInvokeMethod`, inside the actor: the method of the resource named, bound to the call's path.
pub(crate) fn find_method(core: &Core, r: &Req) -> Res {
    let (mut found, snap) = found_of(core, r)?;
    let res = snap
        .pointer(&format!("/resources/{}", r.s("resourceId")))
        .ok_or_else(|| invalid_id("Resource"))?;
    let path = path_of(r, text(res, "path").unwrap_or("/"));
    let segs: Vec<String> = path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    let method = r.s("httpMethod");
    let mut route =
        super::route::rest_resource(res, &segs, method).ok_or_else(|| invalid_id("Method"))?;
    route.validator = text(&route.method, "requestValidatorId")
        .and_then(|v| snap.pointer(&format!("/validators/{v}")))
        .cloned();
    found.path = path.clone();
    found.route = Some(route);
    let req = incoming(r, method, host(r));
    Ok(Out {
        pending: Some(Box::new(Pending::Method(Box::new(found), Box::new(req)))),
        ..Out::default()
    })
}

/// `TestInvokeAuthorizer`, inside the actor: the authorizer as it is now, its cache not used.
pub(crate) fn find_authorizer(core: &Core, r: &Req) -> Res {
    let (mut found, snap) = found_of(core, r)?;
    let id = r.s("authorizerId");
    let mut auth = snap
        .pointer(&format!("/authorizers/{id}"))
        .cloned()
        .ok_or_else(|| invalid_id("Authorizer"))?;
    put(&mut auth, "authorizerResultTtlInSeconds", json!(0));
    let path = path_of(r, "/");
    found.path = path.clone();
    found.route = Some(Route {
        resource_id: String::new(),
        resource_path: path.clone(),
        params: Vec::new(),
        method: json!({"authorizationType": if text(&auth, "type") == Some("COGNITO_USER_POOLS") { "COGNITO_USER_POOLS" } else { "CUSTOM" }, "authorizerId": id}),
        integration: Value::Null,
        authorizer: Some(auth),
        validator: None,
        responses: None,
        integration_responses: None,
    });
    let req = incoming(r, "GET", host(r));
    let given = r
        .value("headers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    Ok(Out {
        pending: Some(Box::new(Pending::Authorizer(
            Box::new(found),
            Box::new(req),
            given,
        ))),
        ..Out::default()
    })
}

/// Serves a test found by the actor: the operation's answer.
pub(crate) async fn run(inner: &Inner, pending: Pending) -> Value {
    match pending {
        Pending::Method(found, req) => method(inner, &found, &req).await,
        Pending::Authorizer(found, req, given) => authorizer(inner, &found, &req, &given).await,
    }
}

/// `Wed Feb 24 00:51:47 UTC 2016`, as AWS's test logs write a time.
pub(crate) fn log_time(ms: i64) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let secs = ms.div_euclid(1000);
    let day = secs.div_euclid(86_400);
    let (y, m, d) = aws_core::time::civil_from_days(day);
    let t = secs.rem_euclid(86_400);
    let wd = usize::try_from(day.rem_euclid(7))
        .ok()
        .and_then(|i| DAYS.get(i))
        .unwrap_or(&"Thu");
    let mon = usize::try_from(m - 1)
        .ok()
        .and_then(|i| MONTHS.get(i))
        .unwrap_or(&"Jan");
    format!(
        "{wd} {mon} {d:02} {:02}:{:02}:{:02} UTC {y}",
        t / 3600,
        t % 3600 / 60,
        t % 60
    )
}

fn log(req: &Incoming, lines: &[String]) -> String {
    let when = log_time(req.now_ms);
    let mut out = format!("Execution log for request {}\n", req.request_id);
    for l in lines {
        out.push_str(&format!("{when} : {l}\n"));
    }
    out
}

fn header_maps(reply: &WebReply) -> (Map<String, Value>, Map<String, Value>) {
    let mut single = Map::new();
    let mut multi: Map<String, Value> = Map::new();
    for (k, v) in &reply.headers {
        single.insert(k.clone(), json!(v));
        if let Some(Value::Array(list)) = multi.get_mut(k) {
            list.push(json!(v));
        } else {
            multi.insert(k.clone(), json!([v]));
        }
    }
    (single, multi)
}

fn millis(started: Instant) -> i64 {
    i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX)
}

async fn method(inner: &Inner, found: &Found, req: &Incoming) -> Value {
    let started = Instant::now();
    let reply = super::rest::test(inner, found, req).await;
    let ms = millis(started);
    let level = Level {
        errors_only: false,
        data_trace: true,
    };
    let lines: Vec<String> =
        execlog::lines(found, req, &reply, u128::try_from(ms).unwrap_or(0), level)
            .into_iter()
            .filter(|l| !(l.starts_with("Extended Request Id") || l.contains("Usage Plan")))
            .collect();
    let (headers, multi) = header_maps(&reply);
    json!({
        "status": reply.status,
        "body": String::from_utf8_lossy(&reply.body),
        "headers": headers,
        "multiValueHeaders": multi,
        "log": log(req, &lines),
        "latency": ms,
    })
}

fn identity_line(given: &Map<String, Value>) -> String {
    let pairs: Vec<String> = given
        .iter()
        .map(|(k, v)| format!("{k}={}", crate::doc::plain(v)))
        .collect();
    format!("Incoming identity: {{{}}}", pairs.join(", "))
}

async fn authorizer(
    inner: &Inner,
    found: &Found,
    req: &Incoming,
    given: &Map<String, Value>,
) -> Value {
    let started = Instant::now();
    let Some(route) = found.route.as_ref() else {
        return json!({});
    };
    let auth = route.authorizer.clone().unwrap_or(Value::Null);
    let id = text(&auth, "id").unwrap_or_default();
    let mut lines = vec![
        format!("Starting authorizer: {id} for request: {}", req.request_id),
        identity_line(given),
    ];
    let mut extra = Context::default();
    let arn = super::rest::method_arn(found, &req.method);
    let cognito = text(&auth, "type") == Some("COGNITO_USER_POOLS");
    let outcome = if cognito {
        super::jwt::cognito(found, route, req, &mut extra)
    } else {
        super::auth::lambda_v1(inner, found, route, req, &arn, &mut extra).await
    };
    let status = match &outcome {
        Ok(()) => {
            lines.push("Successfully completed authorizer execution".to_owned());
            0
        }
        Err(reply) => {
            lines.push(match reply.status {
                401 => format!("Unauthorized request: {}", req.request_id),
                403 => "The client is not authorized to perform this operation.".to_owned(),
                _ => "Execution failed due to configuration error: Authorizer error".to_owned(),
            });
            i64::from(reply.status)
        }
    };
    let mut out =
        json!({"clientStatus": status, "log": log(req, &lines), "latency": millis(started)});
    if cognito {
        // `(memory)`: no Cognito authorizer has been recorded; the identity given and the token's claims are shown.
        let authorization: Map<String, Value> = given
            .iter()
            .map(|(k, v)| (k.clone(), json!([crate::doc::plain(v)])))
            .collect();
        let claims = extra
            .authorizer
            .as_ref()
            .and_then(|a| a.get("claims"))
            .cloned()
            .unwrap_or_else(|| json!({}));
        put(&mut out, "authorization", Value::Object(authorization));
        put(&mut out, "claims", claims);
    } else {
        // Recorded 2026-11-19 (a TOKEN authorizer, allowing and denying): `authorization` is empty `Allow` and `Deny` lists, and
        // there are no `claims`.
        put(&mut out, "authorization", json!({"Allow": [], "Deny": []}));
    }
    if let Some(p) = extra.principal {
        put(&mut out, "principalId", json!(p));
    }
    if let Some(p) = extra.policy {
        put(
            &mut out,
            "policy",
            json!(if p.is_string() {
                crate::doc::plain(&p)
            } else {
                p.to_string()
            }),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_written_as_the_logs_write_them() {
        // 2016-02-24T00:51:47Z, the CLI reference's example.
        assert_eq!(log_time(1_456_275_107_000), "Wed Feb 24 00:51:47 UTC 2016");
    }
}
