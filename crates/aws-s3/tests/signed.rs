//! A gateway that checks signatures, with S3 behind it: a signed PUT, and the same body in the aws-chunked encoding with every chunk
//! signed (what the Java and Go SDKs and the CLI send for large uploads), a wrong chunk signature included.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines
)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use aws_auth::canonical::{Flavor, canonical_request, string_to_sign};
use aws_auth::key::{EMPTY_SHA256, SigningKey, sha256_hex};
use aws_auth::{Credential, Secret, StaticCredentials};
use aws_gateway::{Auth, Config, Gateway};
use aws_s3::S3;
use doze_runtime::{Ctx, SystemClock, Tasks};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const SECRET: &str = "s3cret";
const DATE: &str = "20260101";
const AMZ_DATE: &str = "20260101T000000Z";

struct Signed {
    auth: String,
    seed: String,
}

/// The `Authorization` header for a request with these headers (and the payload hash `hash`), and its signature.
fn sign(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    hash: &str,
) -> Signed {
    let mut b = http::Request::builder()
        .method(method)
        .uri(path)
        .header("host", addr.to_string());
    let mut names = vec!["host".to_owned()];
    for (k, v) in headers {
        b = b.header(*k, *v);
        names.push(k.to_ascii_lowercase());
    }
    names.sort();
    let (parts, ()) = b.body(()).unwrap().into_parts();
    let canonical = canonical_request(
        &parts,
        &names,
        hash,
        Flavor {
            s3: true,
            presigned: false,
        },
    )
    .unwrap();
    let scope = format!("{DATE}/us-east-1/s3/aws4_request");
    let sts = string_to_sign(AMZ_DATE, &scope, &canonical);
    let seed = SigningKey::derive(SECRET, DATE, "us-east-1", "s3")
        .unwrap()
        .sign_hex(sts.as_bytes())
        .unwrap();
    let auth = format!(
        "AWS4-HMAC-SHA256 Credential=test/{scope},SignedHeaders={},Signature={seed}",
        names.join(";")
    );
    Signed { auth, seed }
}

async fn send(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    auth: &str,
    body: &[u8],
) -> (u16, String) {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: {auth}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes()).await.unwrap();
    s.write_all(body).await.unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).await.unwrap();
    let text = String::from_utf8_lossy(&raw).into_owned();
    (
        text.split(' ').nth(1).unwrap().parse().unwrap(),
        text.split("\r\n\r\n").nth(1).unwrap_or_default().to_owned(),
    )
}

/// The body cut into signed chunks, each signature chained to the one before (the seed first), ending in the empty chunk.
fn signed_chunks(seed: &str, parts: &[&[u8]], tamper: bool) -> Vec<u8> {
    let key = SigningKey::derive(SECRET, DATE, "us-east-1", "s3").unwrap();
    let scope = format!("{DATE}/us-east-1/s3/aws4_request");
    let mut prev = seed.to_owned();
    let mut out = Vec::new();
    for (i, part) in parts.iter().chain([&&b""[..]]).enumerate() {
        let sts = format!(
            "AWS4-HMAC-SHA256-PAYLOAD\n{AMZ_DATE}\n{scope}\n{prev}\n{EMPTY_SHA256}\n{}",
            sha256_hex(part)
        );
        prev = key.sign_hex(sts.as_bytes()).unwrap();
        let shown = if tamper && i == 1 {
            "0".repeat(64)
        } else {
            prev.clone()
        };
        out.extend_from_slice(format!("{:x};chunk-signature={shown}\r\n", part.len()).as_bytes());
        out.extend_from_slice(part);
        out.extend_from_slice(b"\r\n");
    }
    out
}

#[tokio::test]
async fn a_signed_put_and_signed_chunks_are_checked() {
    let dir = std::env::temp_dir().join(format!("aws-s3-signed-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    let tasks = Tasks::new();
    let ctx = Arc::new(Ctx {
        dir,
        idle: Duration::from_secs(300),
        tasks: tasks.clone(),
        clock: Arc::new(SystemClock),
        ext: None,
    });
    let mut creds = StaticCredentials::new();
    creds.insert(
        "test",
        Credential {
            secret: Secret::new(SECRET.into()),
            session_token: None,
            expires_at: None,
        },
    );
    let mut config = Config::new("127.0.0.1:0".parse().unwrap());
    config.auth = Auth::Verify(Arc::new(creds));
    config.max_skew_secs = i64::MAX / 4;
    let running = Gateway::new(config)
        .with(Arc::new(S3::new(ctx)))
        .bind()
        .await
        .unwrap();
    let addr = running.local_addr();

    let make = |path: &str| {
        let hash = sha256_hex(b"");
        let h = [
            ("x-amz-content-sha256", hash.as_str()),
            ("x-amz-date", AMZ_DATE),
        ];
        (
            sign(addr, "PUT", path, &h, &hash),
            h.map(|(k, v)| (k.to_owned(), v.to_owned())),
        )
    };
    let (s, h) = make("/bkt");
    let hh: Vec<(&str, &str)> = h.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    assert_eq!(send(addr, "PUT", "/bkt", &hh, &s.auth, b"").await.0, 200);

    // A plain signed PUT with the payload hash in the signature.
    let body = b"signed in one piece";
    let hash = sha256_hex(body);
    let h = [
        ("x-amz-content-sha256", hash.as_str()),
        ("x-amz-date", AMZ_DATE),
    ];
    let s = sign(addr, "PUT", "/bkt/plain", &h, &hash);
    assert_eq!(
        send(addr, "PUT", "/bkt/plain", &h, &s.auth, body).await.0,
        200
    );
    let wrong = [
        ("x-amz-content-sha256", sha256_hex(b"something else")),
        ("x-amz-date", AMZ_DATE.to_owned()),
    ];
    let wrong_ref: Vec<(&str, &str)> = wrong.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let s = sign(addr, "PUT", "/bkt/mismatch", &wrong_ref, &wrong[0].1);
    let (status, text) = send(addr, "PUT", "/bkt/mismatch", &wrong_ref, &s.auth, body).await;
    assert_eq!(status, 400, "{text}");
    assert!(text.contains("XAmzContentSHA256Mismatch"), "{text}");

    // The same kind of body, in signed chunks.
    let parts: [&[u8]; 3] = [b"first chunk, ", b"second chunk, ", b"third"];
    let all: Vec<u8> = parts.concat();
    let len = all.len().to_string();
    let h = [
        ("x-amz-content-sha256", "STREAMING-AWS4-HMAC-SHA256-PAYLOAD"),
        ("x-amz-date", AMZ_DATE),
        ("content-encoding", "aws-chunked"),
        ("x-amz-decoded-content-length", len.as_str()),
    ];
    let s = sign(
        addr,
        "PUT",
        "/bkt/chunked",
        &h,
        "STREAMING-AWS4-HMAC-SHA256-PAYLOAD",
    );
    let ok = send(
        addr,
        "PUT",
        "/bkt/chunked",
        &h,
        &s.auth,
        &signed_chunks(&s.seed, &parts, false),
    )
    .await;
    assert_eq!(ok.0, 200, "{}", ok.1);
    let s = sign(
        addr,
        "PUT",
        "/bkt/tampered",
        &h,
        "STREAMING-AWS4-HMAC-SHA256-PAYLOAD",
    );
    let (status, text) = send(
        addr,
        "PUT",
        "/bkt/tampered",
        &h,
        &s.auth,
        &signed_chunks(&s.seed, &parts, true),
    )
    .await;
    assert_eq!(status, 403, "{text}");
    assert!(text.contains("SignatureDoesNotMatch"), "{text}");

    let hash = sha256_hex(b"");
    let h = [
        ("x-amz-content-sha256", hash.as_str()),
        ("x-amz-date", AMZ_DATE),
    ];
    let read = |path: &'static str| {
        let s = sign(addr, "GET", path, &h, &hash);
        async move { send(addr, "GET", path, &h, &s.auth, b"").await }
    };
    assert_eq!(
        read("/bkt/chunked").await,
        (200, String::from_utf8(all).unwrap())
    );
    assert_eq!(
        read("/bkt/tampered").await.0,
        404,
        "a body whose chunk did not verify is not kept"
    );
    assert_eq!(read("/bkt/plain").await.1, "signed in one piece");
    running.shutdown().await;
    tasks.shutdown().await;
}
