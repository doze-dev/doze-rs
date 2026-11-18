//! One WebSocket connection: `$connect` before it opens, then each message routed and its integration run (a Lambda proxy, an HTTP
//! proxy, or MOCK), the integration's answer sent back when the route has a route response, posts from the `@connections` API
//! written out, and `$disconnect` when it ends. The event a Lambda function receives is AWS's WebSocket event (`requestContext` with
//! `eventType`, `connectionId`, `routeKey`, `messageId`).

use std::sync::Arc;

use aws_wire::service::Socket;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use super::Out;
use super::frame::{In, Reader, encode};
use crate::Inner;
use crate::doc::text;
use crate::plane::event::{Context, clf_time};
use crate::plane::{Find, Found, Incoming, Route, lambda, mapping};

/// What a WebSocket event is made from.
#[derive(Clone, Copy)]
struct Ev<'a> {
    found: &'a Found,
    route: &'a Route,
    id: &'a str,
    kind: &'a str,
    body: Option<&'a str>,
    req: Option<&'a Incoming>,
    connected: i64,
    now: i64,
}

/// The event of a WebSocket route.
fn event(e: &Ev<'_>) -> Value {
    let Ev {
        found,
        route,
        id,
        kind,
        body,
        req,
        connected,
        now,
    } = *e;
    let request_id = aws_wire::RequestId::new().as_str().to_owned();
    let mut ctx = json!({
        "routeKey": route.resource_path, "eventType": kind, "extendedRequestId": request_id, "requestTime": clf_time(now),
        "messageDirection": "IN", "stage": found.stage_name, "connectedAt": connected, "requestTimeEpoch": now,
        "identity": {"sourceIp": "127.0.0.1", "userAgent": req.and_then(|r| r.header("user-agent"))},
        "requestId": request_id, "domainName": req.map(|r| r.host.clone()), "connectionId": id, "apiId": found.api_id(),
    });
    if kind == "MESSAGE" {
        crate::doc::put(&mut ctx, "messageId", json!(super::connection_id()));
    }
    let mut e = json!({"requestContext": ctx, "isBase64Encoded": false});
    if let Some(b) = body {
        crate::doc::put(&mut e, "body", json!(b));
    }
    if let Some(r) = req {
        crate::doc::put(&mut e, "headers", Value::Object(mapping::header_map(r)));
        if !r.query.is_empty() {
            let q: serde_json::Map<String, Value> =
                r.query.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
            crate::doc::put(&mut e, "queryStringParameters", Value::Object(q));
        }
    }
    e
}

/// A Lambda proxy integration of a WebSocket route: the function's `statusCode` and `body`.
async fn proxy(inner: &Inner, found: &Found, route: &Route, ev: &Value) -> (u16, String) {
    let uri = text(&route.integration, "integrationUri").unwrap_or_default();
    let Some(function) = lambda::function_of(uri, found.variables()) else {
        return (500, String::new());
    };
    let arn = format!(
        "arn:aws:execute-api:{}:{}:{}/{}/{}",
        found.region,
        found.account,
        found.api_id(),
        found.stage_name,
        route.resource_path
    );
    match lambda::invoke(
        inner,
        &found.account,
        &function,
        &arn,
        ev.to_string().as_bytes(),
    )
    .await
    {
        Ok(out) if out.error.is_none() => {
            let v: Value = serde_json::from_slice(&out.payload).unwrap_or(Value::Null);
            let status = v
                .get("statusCode")
                .and_then(Value::as_u64)
                .and_then(|s| u16::try_from(s).ok())
                .unwrap_or(200);
            let body = v.get("body").map_or_else(
                || String::from_utf8_lossy(&out.payload).into_owned(),
                crate::doc::plain,
            );
            (status, body)
        }
        Ok(_) => (502, String::new()),
        Err(why) => {
            tracing::warn!(
                "websocket {}: the function could not be invoked: {why}",
                found.api_id()
            );
            (500, String::new())
        }
    }
}

/// Runs a route's integration; its status and body.
async fn integrate(
    inner: &Inner,
    found: &Found,
    route: &Route,
    ev: &Value,
    body: &str,
) -> (u16, String) {
    let i = &route.integration;
    match text(i, "integrationType").unwrap_or_default() {
        "AWS_PROXY" => proxy(inner, found, route, ev).await,
        "MOCK" => {
            let template = route
                .integration_responses
                .as_ref()
                .and_then(Value::as_object)
                .and_then(|m| m.values().next())
                .and_then(|r| r.pointer("/responseTemplates/$default"))
                .and_then(Value::as_str);
            let mut env = crate::plane::vtl::Env {
                input: crate::plane::vtl::Input {
                    body: body.to_owned(),
                    params: json!({}),
                },
                ..Default::default()
            };
            env.vars.insert(
                "context".into(),
                ev.get("requestContext").cloned().unwrap_or(Value::Null),
            );
            let out = template
                .map(|t| crate::plane::vtl::render(t, &mut env).unwrap_or_default())
                .unwrap_or_default();
            (200, out)
        }
        other => {
            tracing::warn!(
                "websocket {}: {other} integrations are not built for WebSocket APIs in doze-aws",
                found.api_id()
            );
            (500, String::new())
        }
    }
}

/// `$connect`: its authorizer and integration, when the API has the route. `Err` is the status the handshake is refused with.
pub(super) async fn connect(
    inner: &Inner,
    found: &Found,
    req: &Incoming,
    id: &str,
) -> Result<(), u16> {
    let Some(route) = found
        .route
        .as_ref()
        .filter(|r| r.resource_path == "$connect")
    else {
        return Ok(());
    };
    match text(&route.method, "authorizationType").unwrap_or("NONE") {
        "AWS_IAM" if req.header("authorization").is_none() => return Err(403),
        "CUSTOM" => {
            let mut extra = Context::default();
            if crate::plane::auth::lambda_v2(inner, found, route, req, &mut extra)
                .await
                .is_err()
            {
                return Err(401);
            }
        }
        _ => {}
    }
    let ev = event(&Ev {
        found,
        route,
        id,
        kind: "CONNECT",
        body: None,
        req: Some(req),
        connected: req.now_ms,
        now: req.now_ms,
    });
    let (status, _) = integrate(inner, found, route, &ev, "").await;
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(status)
    }
}

/// The route key a message selects (`$request.body.action`), or `$default`.
fn select(found: &Found, body: &str) -> String {
    let expr = text(&found.api, "routeSelectionExpression").unwrap_or("$default");
    let expr = expr
        .trim_start_matches("${")
        .trim_end_matches('}')
        .trim_start_matches('$');
    let Some(path) = expr.strip_prefix("request.body") else {
        return "$default".into();
    };
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|doc| mapping::jsonpath(&doc, &format!("${path}")))
        .map(|v| crate::doc::plain(&v))
        .filter(|k| !k.is_empty())
        .unwrap_or_else(|| "$default".into())
}

/// A message: its route found and run, and the answer sent back if the route has a route response.
async fn message(
    inner: &Inner,
    found: &Found,
    id: &str,
    body: &str,
    connected: i64,
) -> Option<String> {
    let key = select(found, body);
    let find = Find {
        api: found.api_id().to_owned(),
        rest: found.stage_name.clone(),
        method: "POST".into(),
        stage_in_path: true,
        route_key: Some(key),
    };
    let request_id = aws_wire::RequestId::new().as_str().to_owned();
    let Ok(found) = crate::plane::locate(inner, Some(&found.region), find).await else {
        return None;
    };
    let Some(route) = found.route.as_ref() else {
        return Some(
            json!({"message": "Forbidden", "connectionId": id, "requestId": request_id})
                .to_string(),
        );
    };
    let now = inner.clock.now_ms();
    let ev = event(&Ev {
        found: &found,
        route,
        id,
        kind: "MESSAGE",
        body: Some(body),
        req: None,
        connected,
        now,
    });
    let (status, answer) = integrate(inner, &found, route, &ev, body).await;
    if status >= 500 {
        return Some(json!({"message": "Internal server error", "connectionId": id, "requestId": request_id}).to_string());
    }
    let replies = text(&route.method, "routeResponseSelectionExpression").is_some()
        || route
            .responses
            .as_ref()
            .and_then(Value::as_object)
            .is_some_and(|m| !m.is_empty());
    replies.then_some(answer)
}

/// The connection, until it ends.
pub(super) async fn run(
    inner: Arc<Inner>,
    found: Found,
    id: String,
    mut rx: mpsc::Receiver<Out>,
    mut socket: Box<dyn Socket>,
    connected: i64,
) {
    let mut reader = Reader::default();
    let mut buf = vec![0u8; 16 * 1024];
    'conn: loop {
        tokio::select! {
            read = socket.read(&mut buf) => {
                let n = match read { Ok(0) | Err(_) => break 'conn, Ok(n) => n };
                if reader.push(buf.get(..n).unwrap_or_default()).is_err() {
                    break 'conn;
                }
                inner.plane.ws.touch(&id, inner.clock.now_ms());
                loop {
                    let msg = match reader.next() { Ok(Some(m)) => m, Ok(None) => break, Err(_) => break 'conn };
                    let out = match msg {
                        In::Text(t) => message(&inner, &found, &id, &t, connected).await.map(|a| encode(1, a.as_bytes())),
                        In::Binary(b) => message(&inner, &found, &id, &String::from_utf8_lossy(&b), connected).await.map(|a| encode(1, a.as_bytes())),
                        In::Ping(p) => Some(encode(10, &p)),
                        In::Pong => None,
                        In::Close => break 'conn,
                    };
                    if let Some(frame) = out && socket.write_all(&frame).await.is_err() {
                        break 'conn;
                    }
                }
            }
            out = rx.recv() => match out {
                Some(Out::Text(t)) => if socket.write_all(&encode(1, t.as_bytes())).await.is_err() { break 'conn },
                Some(Out::Close) | None => break 'conn,
            },
        }
    }
    drop(socket.write_all(&encode(8, &[0x03, 0xe8])).await);
    inner.plane.ws.remove(&id);
    disconnect(&inner, &found, &id, connected).await;
}

async fn disconnect(inner: &Inner, found: &Found, id: &str, connected: i64) {
    let find = Find {
        api: found.api_id().to_owned(),
        rest: found.stage_name.clone(),
        method: "POST".into(),
        stage_in_path: true,
        route_key: Some("$disconnect".into()),
    };
    let Ok(found) = crate::plane::locate(inner, Some(&found.region), find).await else {
        return;
    };
    let Some(route) = found
        .route
        .as_ref()
        .filter(|r| r.resource_path == "$disconnect")
    else {
        return;
    };
    let ev = event(&Ev {
        found: &found,
        route,
        id,
        kind: "DISCONNECT",
        body: None,
        req: None,
        connected,
        now: inner.clock.now_ms(),
    });
    drop(integrate(inner, &found, route, &ev, "").await);
}
