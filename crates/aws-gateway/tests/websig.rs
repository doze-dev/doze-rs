//! The signature on a request to an address a service owns (a function URL with `AWS_IAM`), checked by the gateway over a real
//! socket under both authentication modes.

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
use aws_model_sqs::{meta::META, tables::TABLES};
use aws_schema::meta::ServiceMeta;
use aws_schema::tables::Tables;
use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Call, Reply, Service, Web, WebReply, WebRequest};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const HOST: &str = "abc.lambda-url.us-east-1.localhost";

/// A service that owns `*.lambda-url.*` and answers with what the gateway made of the signature.
#[derive(Debug)]
struct FakeUrls;

impl Service for FakeUrls {
    fn meta(&self) -> &'static ServiceMeta {
        &META
    }

    fn tables(&self) -> &'static Tables {
        &TABLES
    }

    fn call(&self, _call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(async { Err(ApiError::internal("not called")) })
    }

    fn web(&self) -> Option<&dyn Web> {
        Some(self)
    }
}

impl Web for FakeUrls {
    fn owns(&self, host: &str, _path: &str) -> bool {
        host.contains(".lambda-url.")
    }

    fn signing_name(&self, _host: &str, _path: &str) -> Option<&'static str> {
        Some("lambda")
    }

    fn serve(&self, request: WebRequest) -> BoxFuture<'_, WebReply> {
        Box::pin(async move {
            WebReply {
                status: 200,
                headers: Vec::new(),
                body: format!("{:?}", request.signature).into(),
            }
        })
    }
}

fn keys() -> Arc<StaticCredentials> {
    let mut creds = StaticCredentials::new();
    creds.insert(
        "test",
        Credential {
            secret: Secret::new("test-secret".into()),
            session_token: None,
            expires_at: None,
        },
    );
    Arc::new(creds)
}

async fn start(auth: Auth) -> (aws_gateway::Running, SocketAddr) {
    let mut config = Config::new("127.0.0.1:0".parse().unwrap());
    config.auth = auth;
    config.known_keys = keys();
    let running = Gateway::new(config)
        .with(Arc::new(FakeUrls))
        .bind()
        .await
        .unwrap();
    let addr = running.local_addr();
    (running, addr)
}

/// A GET of the URL, signed as an SDK signs it with `secret` for `service`; `None` sends it unsigned.
async fn get(addr: SocketAddr, signer: Option<(&str, &str, &str)>) -> String {
    let mut head = format!("GET / HTTP/1.1\r\nConnection: close\r\nHost: {HOST}\r\n");
    if let Some((key, secret, service)) = signer {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let amz_date = format_amz_date(i64::try_from(now).unwrap());
        let day = amz_date[..8].to_owned();
        let (parts, ()) = http::Request::builder()
            .method("GET")
            .uri("/")
            .header("host", HOST)
            .header("x-amz-date", amz_date.as_str())
            .body(())
            .unwrap()
            .into_parts();
        let signed_headers = vec!["host".to_owned(), "x-amz-date".to_owned()];
        let flavor = Flavor {
            s3: false,
            presigned: false,
        };
        let canonical =
            canonical_request(&parts, &signed_headers, &sha256_hex(b""), flavor).unwrap();
        let scope = format!("{day}/us-east-1/{service}/aws4_request");
        let sts = string_to_sign(&amz_date, &scope, &canonical);
        let sig = SigningKey::derive(secret, &day, "us-east-1", service)
            .unwrap()
            .sign_hex(sts.as_bytes())
            .unwrap();
        head.push_str(&format!("X-Amz-Date: {amz_date}\r\nAuthorization: AWS4-HMAC-SHA256 Credential={key}/{scope},SignedHeaders=host;x-amz-date,Signature={sig}\r\n"));
    }
    head.push_str("\r\n");
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(head.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    let text = String::from_utf8(raw).unwrap();
    text.split_once("\r\n\r\n").unwrap().1.to_owned()
}

#[tokio::test]
async fn under_trust_a_known_key_is_checked_and_an_unknown_one_is_trusted_in_scope() {
    let (running, addr) = start(Auth::Trust).await;
    assert_eq!(get(addr, None).await, "Unsigned");
    let good = get(addr, Some(("test", "test-secret", "lambda"))).await;
    assert!(
        good.starts_with("Signed") && good.contains("verified: true"),
        "{good}"
    );
    let bad = get(addr, Some(("test", "wrong", "lambda"))).await;
    assert!(
        bad.starts_with("Refused(\"The request signature we calculated does not match"),
        "{bad}"
    );
    let other = get(addr, Some(("someone", "anything", "lambda"))).await;
    assert!(other.contains("verified: false"), "{other}");
    let scoped = get(addr, Some(("someone", "anything", "sqs"))).await;
    assert!(
        scoped.contains("Credential should be scoped to correct service: 'lambda'"),
        "{scoped}"
    );
    running.shutdown().await;
}

#[tokio::test]
async fn under_verify_every_key_is_checked() {
    let (running, addr) = start(Auth::Verify(keys())).await;
    let good = get(addr, Some(("test", "test-secret", "lambda"))).await;
    assert!(good.contains("verified: true"), "{good}");
    let unknown = get(addr, Some(("someone", "anything", "lambda"))).await;
    assert!(unknown.starts_with("Refused("), "{unknown}");
    running.shutdown().await;
}
