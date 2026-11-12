//! Lifecycle rules act on exactly the objects they select, and a restart finds the janitor armed without opening any database.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use aws_gateway::{Config, Gateway};
use aws_s3::S3;
use doze_runtime::{Ctx, ManualClock, Tasks};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const AUTH: &str = "AWS4-HMAC-SHA256 Credential=test/20260101/us-east-1/s3/aws4_request, SignedHeaders=host, Signature=00";
const DAY_MS: i64 = 86_400_000;

async fn http(addr: SocketAddr, method: &str, path: &str, extra: &str, body: &str) -> (u16, String) {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: {AUTH}\r\nx-amz-date: 20260101T000000Z\r\nConnection: close\r\n{extra}Content-Length: {}\r\n\r\n",
        body.len()
    );
    s.write_all(head.as_bytes()).await.unwrap();
    s.write_all(body.as_bytes()).await.unwrap();
    let mut raw = String::new();
    s.read_to_string(&mut raw).await.unwrap();
    (raw.split(' ').nth(1).unwrap().parse().unwrap(), raw.split("\r\n\r\n").nth(1).unwrap_or_default().to_owned())
}

const RULES: &str = "<LifecycleConfiguration>\
<Rule><ID>tagged</ID><Status>Enabled</Status><Filter><Tag><Key>t</Key><Value>1</Value></Tag></Filter><Expiration><Days>1</Days></Expiration></Rule>\
<Rule><ID>tmp</ID><Status>Enabled</Status><Filter><Prefix>tmp/</Prefix></Filter><Expiration><Days>1</Days></Expiration></Rule>\
</LifecycleConfiguration>";

#[tokio::test]
async fn a_rule_expires_what_it_selects_and_nothing_else_and_a_restart_keeps_the_janitor_armed() {
    let dir = std::env::temp_dir().join(format!("aws-s3-lifecycle-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    let clock = Arc::new(ManualClock::at(1_700_000_000_000));
    let tasks = Tasks::new();
    let ctx = Arc::new(Ctx { dir: dir.clone(), idle: Duration::from_secs(300), tasks: tasks.clone(), clock: clock.clone(), ext: None });
    let s3 = Arc::new(S3::new(ctx));
    let running = Gateway::new(Config::new("127.0.0.1:0".parse().unwrap())).with(s3.clone()).bind().await.unwrap();
    let addr = running.local_addr();

    http(addr, "PUT", "/life", "", "").await;
    http(addr, "PUT", "/life/a", "x-amz-tagging: t=1\r\n", "tagged").await;
    http(addr, "PUT", "/life/b", "x-amz-tagging: t=2\r\n", "other tag").await;
    http(addr, "PUT", "/life/c", "", "no tag").await;
    http(addr, "PUT", "/life/tmp/d", "", "scratch").await;
    assert_eq!(s3.sweep_now("us-east-1").await.unwrap().actions, 0, "no rule yet, nothing happens");
    assert_eq!(http(addr, "PUT", "/life?lifecycle", "", RULES).await.0, 200);
    assert!(dir.join("lifecycle.armed").exists(), "putting a rule leaves the marker");

    clock.advance(2 * DAY_MS);
    let swept = s3.sweep_now("us-east-1").await.unwrap();
    assert_eq!((swept.actions, swept.armed), (2, true));
    assert_eq!(http(addr, "GET", "/life/a", "", "").await.0, 404, "the tagged object is gone");
    assert_eq!(http(addr, "GET", "/life/tmp/d", "", "").await.0, 404, "so is the one under the prefix");
    assert_eq!(http(addr, "GET", "/life/b", "", "").await.0, 200, "a different tag is not selected");
    assert_eq!(http(addr, "GET", "/life/c", "", "").await.0, 200, "no tag is not selected");

    assert_eq!(http(addr, "DELETE", "/life?lifecycle", "", "").await.0, 204);
    assert!(!dir.join("lifecycle.armed").exists(), "removing the last rule removes the marker");
    running.shutdown().await;
    tasks.shutdown().await;
}
