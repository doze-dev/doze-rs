//! An `http` subscription end to end, over real sockets: the endpoint is sent the `SubscriptionConfirmation`, confirms by fetching its
//! `SubscribeURL`, and is then POSTed every notification with AWS's headers; an `https` one stays pending.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines
)]

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aws_gateway::{Config, Gateway};
use aws_sns::Sns;
use doze_runtime::{Ctx, SystemClock, Tasks};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const AUTH: &str = "AWS4-HMAC-SHA256 Credential=test/20260101/us-east-1/sns/aws4_request, SignedHeaders=host, Signature=00";

/// Reads one HTTP request (head and the body its length promises).
async fn read_request(s: &mut TcpStream) -> String {
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = s.read(&mut buf).await.unwrap();
        raw.extend_from_slice(&buf[..n]);
        let text = String::from_utf8_lossy(&raw).to_string();
        if let Some((head, body)) = text.split_once("\r\n\r\n") {
            let want = head
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .map(str::to_owned)
                })
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
            if body.len() >= want {
                return text;
            }
        }
        if n == 0 {
            return text;
        }
    }
}

/// An endpoint that answers 200 and keeps every request.
async fn endpoint() -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            let request = read_request(&mut s).await;
            log.lock().unwrap().push(request);
            s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
        }
    });
    (port, seen)
}

async fn http(
    addr: SocketAddr,
    method: &str,
    path: &str,
    extra: &str,
    body: &str,
) -> (u16, String) {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n{extra}Content-Length: {}\r\n\r\n",
        body.len()
    );
    s.write_all(head.as_bytes()).await.unwrap();
    s.write_all(body.as_bytes()).await.unwrap();
    let mut raw = String::new();
    s.read_to_string(&mut raw).await.unwrap();
    (
        raw.split(' ').nth(1).unwrap().parse().unwrap(),
        raw.split_once("\r\n\r\n")
            .map(|(_, b)| b.to_owned())
            .unwrap_or_default(),
    )
}

async fn query(addr: SocketAddr, form: &str) -> String {
    let extra = format!(
        "Authorization: {AUTH}\r\nx-amz-date: 20260101T000000Z\r\nContent-Type: application/x-www-form-urlencoded\r\n"
    );
    let (status, body) = http(
        addr,
        "POST",
        "/",
        &extra,
        &format!("{form}&Version=2010-03-31"),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    body
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> &'a str {
    text.split_once(open)
        .unwrap()
        .1
        .split_once(close)
        .unwrap()
        .0
}

async fn wait_for(seen: &Arc<Mutex<Vec<String>>>, n: usize) -> Vec<String> {
    for _ in 0..300 {
        if seen.lock().unwrap().len() >= n {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    seen.lock().unwrap().clone()
}

fn header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request
        .split("\r\n\r\n")
        .next()
        .unwrap()
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(": ")?;
            k.eq_ignore_ascii_case(name).then_some(v)
        })
}

fn body(request: &str) -> Value {
    serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap()
}

/// SNS behind a gateway on a free port.
async fn sns(name: &str) -> (aws_gateway::Running, SocketAddr) {
    let dir = std::env::temp_dir().join(format!("aws-sns-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    let ctx = Arc::new(Ctx {
        dir,
        idle: Duration::from_secs(300),
        tasks: Tasks::new(),
        clock: Arc::new(SystemClock),
        ext: None,
    });
    let running = Gateway::new(Config::new("127.0.0.1:0".parse().unwrap()))
        .with(Arc::new(Sns::new(ctx)))
        .bind()
        .await
        .unwrap();
    let addr = running.local_addr();
    (running, addr)
}

#[tokio::test]
async fn an_http_endpoint_confirms_through_its_subscribe_url_and_is_sent_notifications() {
    let (_running, addr) = sns("http").await;
    let (port, seen) = endpoint().await;

    let created = query(addr, "Action=CreateTopic&Name=orders").await;
    let topic = between(&created, "<TopicArn>", "</TopicArn>").to_owned();
    let hook = format!("http://127.0.0.1:{port}/hook");
    let subscribed = query(
        addr,
        &format!("Action=Subscribe&TopicArn={topic}&Protocol=http&Endpoint={hook}"),
    )
    .await;
    assert!(subscribed.contains("<SubscriptionArn>pending confirmation</SubscriptionArn>"));

    let got = wait_for(&seen, 1).await;
    assert_eq!(got.len(), 1, "the confirmation was POSTed");
    let confirmation = &got[0];
    assert!(confirmation.starts_with("POST /hook HTTP/1.1\r\n"));
    assert_eq!(
        header(confirmation, "x-amz-sns-message-type"),
        Some("SubscriptionConfirmation")
    );
    assert_eq!(
        header(confirmation, "x-amz-sns-topic-arn"),
        Some(topic.as_str())
    );
    assert_eq!(
        header(confirmation, "content-type"),
        Some("text/plain; charset=UTF-8")
    );
    let doc = body(confirmation);
    assert_eq!(doc["Type"], "SubscriptionConfirmation");
    assert_eq!(doc["TopicArn"], topic);
    assert_eq!(doc["Token"].as_str().unwrap().len(), 64);
    let url = doc["SubscribeURL"].as_str().unwrap();
    let path = url.strip_prefix(&format!("http://{addr}")).unwrap();
    assert!(
        path.starts_with("/_aws/sns/confirm?Action=ConfirmSubscription&TopicArn="),
        "{url}"
    );

    // Nothing is sent to an endpoint that has not confirmed.
    query(
        addr,
        &format!("Action=Publish&TopicArn={topic}&Message=early"),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(seen.lock().unwrap().len(), 1);

    // Fetching the URL confirms, unsigned, as on AWS.
    let (status, confirmed) = http(addr, "GET", path, "", "").await;
    assert_eq!(status, 200, "{confirmed}");
    let sub_arn = between(&confirmed, "<SubscriptionArn>", "</SubscriptionArn>").to_owned();
    assert!(sub_arn.starts_with(&format!("{topic}:")));
    // A second fetch finds the token used.
    assert_eq!(http(addr, "GET", path, "", "").await.0, 400);

    query(
        addr,
        &format!("Action=Publish&TopicArn={topic}&Message=shipped&Subject=order"),
    )
    .await;
    let got = wait_for(&seen, 2).await;
    let note = &got[1];
    assert_eq!(header(note, "x-amz-sns-message-type"), Some("Notification"));
    assert_eq!(
        header(note, "x-amz-sns-subscription-arn"),
        Some(sub_arn.as_str())
    );
    let doc = body(note);
    assert_eq!(doc["Type"], "Notification");
    assert_eq!(doc["Message"], "shipped");
    assert_eq!(doc["Subject"], "order");
    assert_eq!(
        header(note, "x-amz-sns-message-id"),
        doc["MessageId"].as_str()
    );
}

#[tokio::test]
async fn an_https_endpoint_is_confirmed_and_sent_notifications_and_a_self_signed_one_is_refused() {
    use aws_webhook::test_server::{Ca, Identity, ok, serve};
    Ca::get().install();
    let (_running, addr) = sns("https").await;
    let server = serve(Ca::get().identity(), ok()).await;
    let created = query(addr, "Action=CreateTopic&Name=secure").await;
    let topic = between(&created, "<TopicArn>", "</TopicArn>").to_owned();
    let hook = format!("https://localhost:{}/secure", server.port);
    let subscribed = query(
        addr,
        &format!("Action=Subscribe&TopicArn={topic}&Protocol=https&Endpoint={hook}"),
    )
    .await;
    assert!(subscribed.contains("pending confirmation"));
    let got = server.seen(1).await;
    assert_eq!(got.len(), 1, "the confirmation was POSTed over TLS");
    assert_eq!(
        header(&got[0], "x-amz-sns-message-type"),
        Some("SubscriptionConfirmation")
    );
    let doc = body(&got[0]);
    let url = doc["SubscribeURL"].as_str().unwrap();
    let path = url.strip_prefix(&format!("http://{addr}")).unwrap();
    let (status, confirmed) = http(addr, "GET", path, "", "").await;
    assert_eq!(status, 200, "{confirmed}");
    query(addr, &format!("Action=Publish&TopicArn={topic}&Message=sealed")).await;
    let got = server.seen(2).await;
    assert_eq!(body(&got[1])["Message"], "sealed");
    assert!(got[1].starts_with("POST /secure HTTP/1.1\r\n"));

    // A server with a self-signed certificate is never sent anything: the handshake is refused (the log names the flags).
    let untrusted = serve(Identity::self_signed(), ok()).await;
    let hook = format!("https://localhost:{}/x", untrusted.port);
    query(
        addr,
        &format!("Action=Subscribe&TopicArn={topic}&Protocol=https&Endpoint={hook}"),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(untrusted.seen(0).await.is_empty());
}
