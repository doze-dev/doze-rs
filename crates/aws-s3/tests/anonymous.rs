//! The anonymous principal, with IAM as the authorizer: an unsigned request reads only what a bucket policy or a public ACL grants
//! to anyone (and not while the public access block restricts the bucket); a website page likewise. In soft mode it is served.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines
)]

mod support;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use aws_gateway::{Config, Gateway, Running};
use aws_peers::{Mode, Peers};
use aws_s3::S3;
use doze_runtime::{Ctx, SystemClock, Tasks};
use support::{Reply, parse, request};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

struct World {
    addr: SocketAddr,
    running: Running,
    tasks: Tasks,
}

async fn world(name: &str, mode: Mode) -> World {
    let dir = std::env::temp_dir().join(format!("aws-s3-anon-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    let tasks = Tasks::new();
    let peers = Peers::new();
    peers.set_mode(mode);
    let ctx = |sub: &str| Ctx {
        dir: dir.join(sub),
        idle: Duration::from_secs(300),
        tasks: tasks.clone(),
        clock: Arc::new(SystemClock),
        ext: Some(peers.clone()),
    };
    let (iam, _) = aws_iam::services(Arc::new(ctx("iam")));
    peers.set_authorizer(Arc::new(iam));
    let running = Gateway::new(Config::new("127.0.0.1:0".parse().unwrap()))
        .with(Arc::new(S3::new(Arc::new(ctx("s3")))))
        .bind()
        .await
        .unwrap();
    World {
        addr: running.local_addr(),
        running,
        tasks,
    }
}

impl World {
    async fn signed(&self, method: &str, path: &str, extra: &[(&str, &str)], body: &str) -> Reply {
        request(self.addr, method, "127.0.0.1", path, extra, body.as_bytes()).await
    }

    /// A request with no signature at all, as curl sends it.
    async fn anon(&self, method: &str, path: &str) -> Reply {
        let mut s = TcpStream::connect(self.addr).await.unwrap();
        let head = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
        );
        s.write_all(head.as_bytes()).await.unwrap();
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).await.unwrap();
        parse(&raw)
    }

    async fn stop(self) {
        self.running.shutdown().await;
        self.tasks.shutdown().await;
    }
}

fn public_read(bucket: &str) -> String {
    format!(
        r#"{{"Version":"2012-10-17","Statement":[{{"Effect":"Allow","Principal":"*","Action":"s3:GetObject","Resource":"arn:aws:s3:::{bucket}/*"}}]}}"#
    )
}

const OPEN: &str = "<PublicAccessBlockConfiguration><BlockPublicAcls>false</BlockPublicAcls><IgnorePublicAcls>false</IgnorePublicAcls><BlockPublicPolicy>false</BlockPublicPolicy><RestrictPublicBuckets>false</RestrictPublicBuckets></PublicAccessBlockConfiguration>";
const RESTRICT: &str = "<PublicAccessBlockConfiguration><RestrictPublicBuckets>true</RestrictPublicBuckets></PublicAccessBlockConfiguration>";

#[tokio::test]
async fn under_enforce_anonymous_reads_only_what_a_policy_grants_to_anyone() {
    let w = world("policy", Mode::Enforce).await;
    assert_eq!(w.signed("PUT", "/pub", &[], "").await.status, 200);
    assert_eq!(w.signed("PUT", "/pub/k", &[], "hello").await.status, 200);

    let denied = w.anon("GET", "/pub/k").await;
    assert_eq!(denied.status, 403, "{}", denied.text());
    assert!(
        denied
            .text()
            .contains("<Code>AccessDenied</Code><Message>Access Denied</Message>"),
        "{}",
        denied.text()
    );
    assert_eq!(w.anon("PUT", "/pub/other").await.status, 403);

    // A new bucket blocks public policies; once the block is off the policy lets anyone read.
    let blocked = w
        .signed("PUT", "/pub?policy", &[], &public_read("pub"))
        .await;
    assert_eq!(blocked.status, 403, "{}", blocked.text());
    assert_eq!(
        w.signed("PUT", "/pub?publicAccessBlock", &[], OPEN)
            .await
            .status,
        200
    );
    assert_eq!(
        w.signed("PUT", "/pub?policy", &[], &public_read("pub"))
            .await
            .status,
        204
    );
    let read = w.anon("GET", "/pub/k").await;
    assert_eq!((read.status, read.text().as_str()), (200, "hello"));
    // The policy grants reads only.
    assert_eq!(w.anon("GET", "/pub?list-type=2").await.status, 403);

    // RestrictPublicBuckets: a public policy lets only the owner's account in.
    assert_eq!(
        w.signed("PUT", "/pub?publicAccessBlock", &[], RESTRICT)
            .await
            .status,
        200
    );
    assert_eq!(w.anon("GET", "/pub/k").await.status, 403);
    assert_eq!(
        w.signed("GET", "/pub/k", &[], "").await.status,
        200,
        "a signed caller is the account"
    );
    w.stop().await;
}

#[tokio::test]
async fn a_public_read_acl_lets_anonymous_read_that_object() {
    let w = world("acl", Mode::Enforce).await;
    w.signed(
        "PUT",
        "/acl",
        &[("x-amz-object-ownership", "ObjectWriter")],
        "",
    )
    .await;
    assert_eq!(
        w.signed("DELETE", "/acl?publicAccessBlock", &[], "")
            .await
            .status,
        204
    );
    assert_eq!(
        w.signed("PUT", "/acl/open", &[("x-amz-acl", "public-read")], "seen")
            .await
            .status,
        200
    );
    w.signed("PUT", "/acl/closed", &[], "unseen").await;
    let open = w.anon("GET", "/acl/open").await;
    assert_eq!((open.status, open.text().as_str()), (200, "seen"));
    assert_eq!(w.anon("GET", "/acl/closed").await.status, 403);
    // IgnorePublicAcls takes it back.
    let ignore = "<PublicAccessBlockConfiguration><IgnorePublicAcls>true</IgnorePublicAcls></PublicAccessBlockConfiguration>";
    w.signed("PUT", "/acl?publicAccessBlock", &[], ignore).await;
    assert_eq!(w.anon("GET", "/acl/open").await.status, 403);
    w.stop().await;
}

#[tokio::test]
async fn a_website_is_served_only_once_it_is_public() {
    let w = world("site", Mode::Enforce).await;
    w.signed("PUT", "/site", &[], "").await;
    w.signed(
        "PUT",
        "/site/index.html",
        &[("content-type", "text/html")],
        "<h1>hi</h1>",
    )
    .await;
    let website = "<WebsiteConfiguration><IndexDocument><Suffix>index.html</Suffix></IndexDocument></WebsiteConfiguration>";
    assert_eq!(
        w.signed("PUT", "/site?website", &[], website).await.status,
        200
    );
    let refused = w.anon("GET", "/_aws/s3-website/site/").await;
    assert_eq!(refused.status, 403, "{}", refused.text());
    w.signed("PUT", "/site?publicAccessBlock", &[], OPEN).await;
    w.signed("PUT", "/site?policy", &[], &public_read("site"))
        .await;
    let page = w.anon("GET", "/_aws/s3-website/site/").await;
    assert_eq!((page.status, page.text().as_str()), (200, "<h1>hi</h1>"));
    w.stop().await;
}

#[tokio::test]
async fn under_soft_anonymous_is_served_and_the_refusal_is_only_logged() {
    let w = world("soft", Mode::Soft).await;
    w.signed("PUT", "/soft", &[], "").await;
    w.signed("PUT", "/soft/k", &[], "served").await;
    let read = w.anon("GET", "/soft/k").await;
    assert_eq!((read.status, read.text().as_str()), (200, "served"));
    w.stop().await;
}
