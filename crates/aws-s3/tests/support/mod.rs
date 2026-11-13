//! The pieces the HTTP tests share: a gateway over a real socket with S3 behind it, and a raw HTTP client.

#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use aws_gateway::{Config, Gateway, Running};
use aws_s3::S3;
use doze_runtime::{Ctx, SystemClock, Tasks};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub(crate) mod lab;

pub(crate) struct Server {
    running: Running,
    pub(crate) tasks: Tasks,
    pub(crate) addr: SocketAddr,
}

pub(crate) async fn start(dir: &Path) -> Server {
    let tasks = Tasks::new();
    let ctx = Arc::new(Ctx {
        dir: dir.to_path_buf(),
        idle: Duration::from_secs(300),
        tasks: tasks.clone(),
        clock: Arc::new(SystemClock),
        ext: None,
    });
    let running = Gateway::new(Config::new("127.0.0.1:0".parse().unwrap()))
        .with(Arc::new(S3::new(ctx)))
        .bind()
        .await
        .unwrap();
    let addr = running.local_addr();
    Server {
        running,
        tasks,
        addr,
    }
}

impl Server {
    pub(crate) async fn stop(self) {
        self.running.shutdown().await;
        self.tasks.shutdown().await;
    }
}

pub(crate) struct Reply {
    pub(crate) status: u16,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
}

impl Reply {
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub(crate) fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

pub(crate) const AUTH: &str = "AWS4-HMAC-SHA256 Credential=test/20260101/us-east-1/s3/aws4_request, SignedHeaders=host, Signature=00";

pub(crate) fn head(
    method: &str,
    host: &str,
    path: &str,
    extra: &[(&str, &str)],
    len: usize,
) -> String {
    let mut r = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: {AUTH}\r\nx-amz-date: 20260101T000000Z\r\nConnection: close\r\nContent-Length: {len}\r\n"
    );
    for (k, v) in extra {
        r.push_str(&format!("{k}: {v}\r\n"));
    }
    r.push_str("\r\n");
    r
}

pub(crate) fn parse(raw: &[u8]) -> Reply {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("a response head");
    let text = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = text.lines();
    let status = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect();
    Reply {
        status,
        headers,
        body: raw[split + 4..].to_vec(),
    }
}

pub(crate) async fn request(
    addr: SocketAddr,
    method: &str,
    host: &str,
    path: &str,
    extra: &[(&str, &str)],
    body: &[u8],
) -> Reply {
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(head(method, host, path, extra, body.len()).as_bytes())
        .await
        .unwrap();
    s.write_all(body).await.unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).await.unwrap();
    parse(&raw)
}

pub(crate) async fn call(
    srv: &Server,
    method: &str,
    path: &str,
    extra: &[(&str, &str)],
    body: &[u8],
) -> Reply {
    request(srv.addr, method, "127.0.0.1", path, extra, body).await
}

pub(crate) fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aws-s3-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    dir
}
