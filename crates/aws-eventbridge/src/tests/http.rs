//! A target that is an API destination makes one HTTP request, with the connection's credentials; what cannot be called is kept and
//! skipped.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::World;

/// A server that answers `status` to everything and keeps each request (head and body) as text.
fn server(w: &World, status: u16) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = w.rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    w.rt.spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else {
                return;
            };
            let mut raw = Vec::new();
            let mut buf = [0u8; 4096];
            // Read until the head is in and as many body bytes as it promises.
            loop {
                let n = s.read(&mut buf).await.unwrap();
                raw.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&raw).to_string();
                let Some((head, body)) = text.split_once("\r\n\r\n") else {
                    continue;
                };
                let want = head
                    .lines()
                    .find_map(|l| l.strip_prefix("Content-Length: "))
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(0);
                if body.len() >= want || n == 0 {
                    break;
                }
            }
            log.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&raw).to_string());
            let reply =
                format!("HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            s.write_all(reply.as_bytes()).await.unwrap();
        }
    });
    (port, seen)
}

/// A rule on the default bus delivering to a new API destination at `endpoint`, over a connection; returns the destination's ARN.
fn wired(w: &mut World, auth: &Value, endpoint: &str, target_extra: &Value) -> String {
    let conn = w.ok(
        "CreateConnection",
        &json!({"Name": "c", "AuthorizationType": auth["type"], "AuthParameters": auth["params"]}),
    );
    let dest = w.ok(
        "CreateApiDestination",
        &json!({"Name": "d", "ConnectionArn": conn["ConnectionArn"], "InvocationEndpoint": endpoint, "HttpMethod": "POST"}),
    );
    w.rule("default", "r", &json!({"source": ["x"]}));
    let mut target = json!({"Id": "t", "Arn": dest["ApiDestinationArn"]});
    for (k, v) in target_extra.as_object().unwrap() {
        target[k] = v.clone();
    }
    w.ok("PutTargets", &json!({"Rule": "r", "Targets": [target]}));
    dest["ApiDestinationArn"].as_str().unwrap().to_owned()
}

fn basic() -> Value {
    json!({"type": "BASIC", "params": {"BasicAuthParameters": {"Username": "ada", "Password": "hunter2"},
        "InvocationHttpParameters": {"HeaderParameters": [{"Key": "X-Conn", "Value": "from-connection"}],
            "QueryStringParameters": [{"Key": "q", "Value": "2"}], "BodyParameters": [{"Key": "extra", "Value": "yes"}]}}})
}

#[test]
fn the_request_carries_the_path_the_headers_the_query_the_credentials_and_the_event() {
    let mut w = World::new();
    let (port, seen) = server(&w, 200);
    let extra = json!({"HttpParameters": {"PathParameterValues": ["orders"], "QueryStringParameters": {"a": "1"}, "HeaderParameters": {"X-T": "v"}}});
    wired(
        &mut w,
        &basic(),
        &format!("http://127.0.0.1:{port}/hooks/*/run"),
        &extra,
    );
    w.put("default", "x", &json!({"n": 1}));
    let requests = seen.lock().unwrap().clone();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert!(
        request.starts_with("POST /hooks/orders/run?a=1&q=2 HTTP/1.1\r\n"),
        "{request}"
    );
    assert!(
        request.contains("Authorization: Basic YWRhOmh1bnRlcjI=\r\n"),
        "ada:hunter2"
    );
    assert!(request.contains("X-T: v\r\n") && request.contains("X-Conn: from-connection\r\n"));
    assert!(request.contains("Content-Type: application/json\r\n"));
    let body: Value = serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body["source"], "x");
    assert_eq!(body["detail"], json!({"n": 1}));
    assert_eq!(
        body["extra"], "yes",
        "the connection's body parameter joins an object"
    );
}

#[test]
fn an_api_key_is_a_header_and_a_get_has_no_body() {
    let mut w = World::new();
    let (port, seen) = server(&w, 204);
    let key = json!({"type": "API_KEY", "params": {"ApiKeyAuthParameters": {"ApiKeyName": "x-api-key", "ApiKeyValue": "s3cret"}}});
    wired(
        &mut w,
        &key,
        &format!("http://127.0.0.1:{port}/ping"),
        &json!({}),
    );
    w.ok(
        "UpdateApiDestination",
        &json!({"Name": "d", "HttpMethod": "GET"}),
    );
    w.put("default", "x", &json!({}));
    let requests = seen.lock().unwrap().clone();
    assert!(requests[0].starts_with("GET /ping HTTP/1.1\r\n"));
    assert!(requests[0].contains("x-api-key: s3cret\r\n"));
    assert!(requests[0].ends_with("\r\n\r\n"), "no body");
    assert!(w.core.retries.is_empty());
}

#[test]
fn a_server_error_is_retried_and_a_refusal_is_not() {
    let mut w = World::new();
    let (port, seen) = server(&w, 503);
    wired(
        &mut w,
        &basic(),
        &format!("http://127.0.0.1:{port}/x"),
        &json!({}),
    );
    w.put("default", "x", &json!({}));
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(w.core.retries.len(), 1, "a 503 may pass");
    let (port, _) = server(&w, 403);
    w.ok(
        "UpdateApiDestination",
        &json!({"Name": "d", "InvocationEndpoint": format!("http://127.0.0.1:{port}/x")}),
    );
    w.core.retries.clear();
    w.put("default", "x", &json!({}));
    assert!(w.core.retries.is_empty(), "a 403 will not");
    // Nothing listening is a failure that may pass.
    w.ok(
        "UpdateApiDestination",
        &json!({"Name": "d", "InvocationEndpoint": "http://127.0.0.1:1/x"}),
    );
    w.put("default", "x", &json!({}));
    assert_eq!(w.core.retries.len(), 1);
}

#[test]
fn what_cannot_be_called_is_kept_and_skipped() {
    let mut w = World::new();
    let oauth = json!({"type": "OAUTH_CLIENT_CREDENTIALS", "params": {"OAuthParameters": {
        "ClientParameters": {"ClientID": "i", "ClientSecret": "s"}, "AuthorizationEndpoint": "http://127.0.0.1:1/t", "HttpMethod": "POST"}}});
    wired(&mut w, &oauth, "http://127.0.0.1:1/x", &json!({}));
    let put = w.put("default", "x", &json!({}));
    assert_eq!(put["FailedEntryCount"], 0);
    assert!(w.core.retries.is_empty());
    // https needs a TLS client, which doze-aws does not carry.
    w.ok("DeleteApiDestination", &json!({"Name": "d"}));
    w.ok("DeleteConnection", &json!({"Name": "c"}));
    wired(&mut w, &basic(), "https://example.test/hook", &json!({}));
    w.put("default", "x", &json!({}));
    assert!(w.core.retries.is_empty());
}
