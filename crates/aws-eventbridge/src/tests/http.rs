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

/// Runs the runtime until no API destination call is under way and `seen` has `n` requests (the calls are made off the actor).
fn settle(w: &World, seen: &Arc<Mutex<Vec<String>>>, n: usize) {
    w.rt.block_on(async {
        for _ in 0..500 {
            if w.core.calls.under_way() == 0 && seen.lock().unwrap().len() >= n {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("the API destination calls did not settle");
    });
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
    settle(&w, &seen, 1);
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
    settle(&w, &seen, 1);
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
    // One retry allowed: the call is made twice, a second apart, by the task and not the actor.
    let arn = w.ok("DescribeApiDestination", &json!({"Name": "d"}))["ApiDestinationArn"].clone();
    w.ok(
        "PutTargets",
        &json!({"Rule": "r", "Targets": [{"Id": "t", "Arn": arn, "RetryPolicy": {"MaximumRetryAttempts": 1}}]}),
    );
    w.put("default", "x", &json!({}));
    settle(&w, &seen, 2);
    let tries: Vec<(u32, String, bool)> = w
        .core
        .calls
        .attempts()
        .into_iter()
        .map(|a| (a.try_number, a.result, a.retrying))
        .collect();
    assert_eq!(
        tries,
        [
            (1, "HTTP503".to_owned(), true),
            (2, "HTTP503".to_owned(), false)
        ],
        "a 503 may pass, once"
    );
    assert!(
        w.core.retries.is_empty(),
        "the actor holds no retry of its own"
    );
    let (port, refused) = server(&w, 403);
    w.ok(
        "UpdateApiDestination",
        &json!({"Name": "d", "InvocationEndpoint": format!("http://127.0.0.1:{port}/x")}),
    );
    w.put("default", "x", &json!({}));
    settle(&w, &refused, 1);
    let last = w.core.calls.attempts().pop().unwrap();
    assert_eq!(
        (last.result.as_str(), last.retrying),
        ("HTTP403", false),
        "a 403 will not"
    );
    // Nothing listening is a failure that may pass.
    w.ok(
        "UpdateApiDestination",
        &json!({"Name": "d", "InvocationEndpoint": "http://127.0.0.1:1/x"}),
    );
    w.put("default", "x", &json!({}));
    w.rt.block_on(async { tokio::time::sleep(std::time::Duration::from_millis(100)).await });
    let last = w.core.calls.attempts().pop().unwrap();
    assert_eq!(
        (last.result.as_str(), last.retrying),
        ("ConnectionFailed", true)
    );
}

#[test]
fn a_slow_endpoint_does_not_hold_the_region() {
    let mut w = World::new();
    // A server that takes the connection and never answers.
    let listener = w.rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
    let port = listener.local_addr().unwrap().port();
    w.rt.spawn(async move {
        let mut held = Vec::new();
        while let Ok((s, _)) = listener.accept().await {
            held.push(s);
        }
    });
    wired(
        &mut w,
        &basic(),
        &format!("http://127.0.0.1:{port}/slow"),
        &json!({}),
    );
    let started = std::time::Instant::now();
    let put = w.put("default", "x", &json!({}));
    assert_eq!(put["FailedEntryCount"], 0);
    w.ok("ListRules", &json!({}));
    assert!(
        started.elapsed() < std::time::Duration::from_secs(1),
        "the actor answered while the call waits"
    );
    assert_eq!(w.core.calls.under_way(), 1);
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
}

#[test]
fn an_https_destination_is_called_over_tls_and_an_untrusted_certificate_is_a_failed_connection() {
    use aws_webhook::test_server::{Ca, Identity, ok, serve};
    Ca::get().install();
    let mut w = World::new();
    let server = w.rt.block_on(serve(Ca::get().identity(), ok()));
    wired(
        &mut w,
        &basic(),
        &format!("https://localhost:{}/secure/*", server.port),
        &json!({"HttpParameters": {"PathParameterValues": ["a b"]}}),
    );
    w.put("default", "x", &json!({"n": 2}));
    let requests = w.rt.block_on(server.seen(1));
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].starts_with("POST /secure/a%20b?q=2 HTTP/1.1\r\n"),
        "{}",
        requests[0]
    );
    assert!(requests[0].contains("Authorization: Basic YWRhOmh1bnRlcjI=\r\n"));
    w.rt.block_on(async { tokio::time::sleep(std::time::Duration::from_millis(50)).await });
    assert_eq!(w.core.calls.attempts().pop().unwrap().result, "200");

    // A server whose certificate nothing trusts gets no request; the attempt fails as a connection does, and may pass.
    let untrusted = w.rt.block_on(serve(Identity::self_signed(), ok()));
    w.ok(
        "UpdateApiDestination",
        &json!({"Name": "d", "InvocationEndpoint": format!("https://localhost:{}/x", untrusted.port)}),
    );
    w.put("default", "x", &json!({}));
    w.rt.block_on(async { tokio::time::sleep(std::time::Duration::from_millis(200)).await });
    let last = w.core.calls.attempts().pop().unwrap();
    assert_eq!(
        (last.result.as_str(), last.retrying),
        ("ConnectionFailed", true)
    );
    assert!(w.rt.block_on(untrusted.seen(0)).is_empty());
}
