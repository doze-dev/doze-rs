//! The gateway over a real socket, with a fake SQS behind it: routing, both authentication modes, validation, errors, and shutdown.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::net::SocketAddr;
use std::sync::Arc;

use aws_auth::canonical::{Flavor, canonical_request, string_to_sign};
use aws_auth::key::{SigningKey, sha256_hex};
use aws_auth::{Credential, Secret, StaticCredentials};
use aws_core::time::format_amz_date;
use aws_gateway::{Auth, Config, Gateway};
use aws_model_sqs::keys::{ListQueuesRequest as LQRq, ListQueuesResult as LQ};
use aws_model_sqs::{meta::META, tables::TABLES};
use aws_schema::build::Object;
use aws_schema::meta::ServiceMeta;
use aws_schema::tables::Tables;
use aws_schema::view::View;
use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Call, Output, Reply, Service};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug)]
struct FakeSqs;

impl Service for FakeSqs {
    fn meta(&self) -> &'static ServiceMeta {
        &META
    }

    fn tables(&self) -> &'static Tables {
        &TABLES
    }

    fn call(&self, call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(async move {
            let name = TABLES.op(call.op).unwrap().name;
            match name {
                "ListQueues" => {
                    let view = View::<LQRq>::root(&call.arena, &call.body, call.root).unwrap();
                    let prefix = view
                        .get(LQRq::QUEUE_NAME_PREFIX)
                        .unwrap_or("none")
                        .to_owned();
                    let mut arena = aws_schema::arena::Arena::new();
                    let mut out = Object::<LQ>::root(&mut arena);
                    out.put_list(LQ::QUEUE_URLS, |l| {
                        l.push(&format!("http://sqs/{}/{prefix}", call.caller.account_id));
                    });
                    let root = out.finish();
                    Ok(Output { arena, root })
                }
                _ => Err(ApiError::modeled(
                    "QueueDoesNotExist",
                    "The specified queue does not exist.",
                )),
            }
        })
    }
}

async fn start(auth: Auth) -> (aws_gateway::Running, SocketAddr) {
    let mut config = Config::new("127.0.0.1:0".parse().unwrap());
    config.auth = auth;
    let running = Gateway::new(config)
        .with(Arc::new(FakeSqs))
        .bind()
        .await
        .unwrap();
    let addr = running.local_addr();
    (running, addr)
}

/// One request, one response: the status, the headers as text, and the body.
async fn send(addr: SocketAddr, headers: &[(&str, &str)], body: &str) -> (u16, String, String) {
    let mut request = format!(
        "POST / HTTP/1.1\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
        request.push_str(&format!("Host: {addr}\r\n"));
    }
    for (k, v) in headers {
        request.push_str(&format!("{k}: {v}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    let text = String::from_utf8(raw).unwrap();
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    (status, head.to_ascii_lowercase(), body.to_owned())
}

const SCOPE: &str = "Credential=test/20130524/us-east-1/sqs/aws4_request,SignedHeaders=host;x-amz-date,Signature=00";

fn trusting_headers<'a>(target: &'a str, auth: &'a str) -> Vec<(&'a str, &'a str)> {
    vec![
        ("X-Amz-Target", target),
        ("X-Amz-Date", "20130524T000000Z"),
        ("Authorization", auth),
    ]
}

#[tokio::test]
async fn a_trusted_request_is_routed_decoded_called_and_encoded() {
    let (running, addr) = start(Auth::Trust).await;
    let auth = format!("AWS4-HMAC-SHA256 {SCOPE}");
    let (status, head, body) = send(
        addr,
        &trusting_headers("AmazonSQS.ListQueues", &auth),
        r#"{"QueueNamePrefix":"jobs"}"#,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(
        head.contains("content-type: application/x-amz-json-1.0"),
        "{head}"
    );
    assert!(head.contains("x-amzn-requestid: "), "{head}");
    assert_eq!(body, r#"{"QueueUrls":["http://sqs/000000000000/jobs"]}"#);
    running.shutdown().await;
}

#[tokio::test]
async fn an_empty_body_is_an_empty_object() {
    let (running, addr) = start(Auth::Trust).await;
    let auth = format!("AWS4-HMAC-SHA256 {SCOPE}");
    let (status, _, body) = send(addr, &trusting_headers("AmazonSQS.ListQueues", &auth), "").await;
    assert_eq!(
        (status, body.as_str()),
        (200, r#"{"QueueUrls":["http://sqs/000000000000/none"]}"#)
    );
    running.shutdown().await;
}

#[tokio::test]
async fn a_modelled_error_carries_its_codes_from_the_model() {
    let (running, addr) = start(Auth::Trust).await;
    let auth = format!("AWS4-HMAC-SHA256 {SCOPE}");
    let (status, head, body) = send(
        addr,
        &trusting_headers("AmazonSQS.GetQueueUrl", &auth),
        r#"{"QueueName":"q"}"#,
    )
    .await;
    assert_eq!(status, 400);
    assert!(
        head.contains("x-amzn-query-error: aws.simplequeueservice.nonexistentqueue;sender"),
        "{head}"
    );
    assert_eq!(
        body,
        r#"{"__type":"com.amazonaws.sqs#QueueDoesNotExist","message":"The specified queue does not exist."}"#
    );
    running.shutdown().await;
}

#[tokio::test]
async fn a_body_the_model_refuses_never_reaches_the_service() {
    let (running, addr) = start(Auth::Trust).await;
    let auth = format!("AWS4-HMAC-SHA256 {SCOPE}");
    // QueueNamePrefix is a string, not a number; and truncated JSON is a syntax error.
    for body in [r#"{"QueueNamePrefix":5}"#, r#"{"QueueNamePrefix":"#] {
        let (status, _, answer) =
            send(addr, &trusting_headers("AmazonSQS.ListQueues", &auth), body).await;
        assert_eq!(status, 400, "{body}");
        assert!(answer.contains("ValidationException"), "{answer}");
    }
    running.shutdown().await;
}

#[tokio::test]
async fn requests_that_cannot_be_routed_are_refused_by_name() {
    let (running, addr) = start(Auth::Trust).await;
    let auth = format!("AWS4-HMAC-SHA256 {SCOPE}");
    // No signature at all.
    let (status, _, body) = send(addr, &[("X-Amz-Target", "AmazonSQS.ListQueues")], "{}").await;
    assert_eq!(status, 400);
    assert!(body.contains("MissingAuthenticationToken"), "{body}");
    // A service this gateway does not have.
    let other = "AWS4-HMAC-SHA256 Credential=test/20130524/us-east-1/s3/aws4_request,SignedHeaders=host;x-amz-date,Signature=00";
    let (status, _, body) =
        send(addr, &trusting_headers("AmazonSQS.ListQueues", other), "{}").await;
    assert_eq!(status, 400);
    assert!(body.contains("UnrecognizedClientException"), "{body}");
    // An operation the model does not have.
    let (status, _, body) = send(addr, &trusting_headers("AmazonSQS.Nope", &auth), "{}").await;
    assert_eq!(status, 404);
    assert!(body.contains("UnknownOperationException"), "{body}");
    running.shutdown().await;
}

#[tokio::test]
async fn a_query_client_of_a_json_service_is_told_so_in_xml() {
    let (running, addr) = start(Auth::Trust).await;
    let auth = format!("AWS4-HMAC-SHA256 {SCOPE}");
    let headers = [
        ("Content-Type", "application/x-www-form-urlencoded"),
        ("X-Amz-Date", "20130524T000000Z"),
        ("Authorization", auth.as_str()),
    ];
    let (status, head, body) = send(addr, &headers, "Action=ListQueues&Version=2012-11-05").await;
    assert_eq!(status, 400, "{body}");
    assert!(head.contains("content-type: text/xml"), "{head}");
    assert!(
        body.starts_with(
            "<ErrorResponse><Error><Type>Sender</Type><Code>UnsupportedProtocol</Code>"
        ) && body.contains("Upgrade the SDK or CLI"),
        "{body}"
    );
    running.shutdown().await;
}

/// A request signed the way an SDK signs it, with `secret`.
fn signed(
    addr: SocketAddr,
    secret: &str,
    access_key: &str,
    target: &str,
    body: &str,
) -> Vec<(String, String)> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let amz_date = format_amz_date(i64::try_from(now).unwrap());
    let day = amz_date[..8].to_owned();
    let amz_date = amz_date.as_str();
    let host = addr.to_string();
    let host = host.as_str();
    let hash = sha256_hex(body.as_bytes());
    let (parts, ()) = http::Request::builder()
        .method("POST")
        .uri("/")
        .header("host", host)
        .header("x-amz-date", amz_date)
        .header("x-amz-target", target)
        .body(())
        .unwrap()
        .into_parts();
    let signed_headers: Vec<String> = ["host", "x-amz-date", "x-amz-target"]
        .map(String::from)
        .to_vec();
    let canonical = canonical_request(
        &parts,
        &signed_headers,
        &hash,
        Flavor {
            s3: false,
            presigned: false,
        },
    )
    .unwrap();
    let scope = format!("{day}/us-east-1/sqs/aws4_request");
    let sts = string_to_sign(amz_date, &scope, &canonical);
    let sig = SigningKey::derive(secret, &day, "us-east-1", "sqs")
        .unwrap()
        .sign_hex(sts.as_bytes())
        .unwrap();
    let auth = format!(
        "AWS4-HMAC-SHA256 Credential={access_key}/{scope},SignedHeaders=host;x-amz-date;x-amz-target,Signature={sig}"
    );
    vec![
        ("Host".into(), host.to_owned()),
        ("X-Amz-Date".into(), amz_date.to_owned()),
        ("X-Amz-Target".into(), target.into()),
        ("Authorization".into(), auth),
    ]
}

#[tokio::test]
async fn in_verify_mode_the_signature_is_checked() {
    let mut creds = StaticCredentials::new();
    creds.insert(
        "test",
        Credential {
            secret: Secret::new("test-secret".into()),
            session_token: None,
            expires_at: None,
        },
    );
    let (running, addr) = start(Auth::Verify(Arc::new(creds))).await;
    let body = r#"{"QueueNamePrefix":"a"}"#;
    let call = |secret: &str, key: &str| {
        let owned = signed(addr, secret, key, "AmazonSQS.ListQueues", body);
        async move {
            let headers: Vec<(&str, &str)> = owned
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            send(addr, &headers, body).await
        }
    };
    // A request signed with the right secret goes through.
    let (status, _, answer) = call("test-secret", "test").await;
    assert_eq!(
        (status, answer.as_str()),
        (200, r#"{"QueueUrls":["http://sqs/000000000000/a"]}"#)
    );
    // The wrong secret does not, and says so the way the Coral front end does.
    let (status, _, answer) = call("another-secret", "test").await;
    assert_eq!(status, 400);
    assert!(
        answer.contains("InvalidSignatureException") && answer.contains("does not match"),
        "{answer}"
    );
    // An access key nobody has.
    let (status, _, answer) = call("test-secret", "nobody").await;
    assert_eq!(status, 400);
    assert!(answer.contains("UnrecognizedClientException"), "{answer}");
    running.shutdown().await;
}
