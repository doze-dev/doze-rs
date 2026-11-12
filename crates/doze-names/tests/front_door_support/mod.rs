//! What the front-door tests share: a private home, free loopback ports, a backend that says who it is, raw HTTP and
//! HTTPS exchanges.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_names::Registry;
use doze_names::front_door::FrontConfig;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub(crate) fn scratch(tag: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let i = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("doze-names-front-{tag}-{}-{i}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub(crate) fn free() -> String {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string()
}

/// A backend that answers "<who> <Host header>", and on an upgrade request switches and echoes.
pub(crate) async fn backend(who: &'static str) -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = l.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let host = req.lines().find_map(|l| {
                    l.strip_prefix("host: ")
                        .or_else(|| l.strip_prefix("Host: "))
                });
                if req.to_ascii_lowercase().contains("upgrade: websocket") {
                    let _ = s.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n").await;
                    let (mut r, mut w) = s.split();
                    let _ = tokio::io::copy(&mut r, &mut w).await;
                    return;
                }
                let body = format!("{who} {}", host.unwrap_or("?"));
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes()).await;
            });
        }
    });
    addr
}

/// A raw HTTP/1.1 exchange over any stream: status line and body.
pub(crate) async fn exchange<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    s: &mut S,
    host: &str,
) -> (String, String) {
    let req = format!("GET /x?y=1 HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).await.unwrap();
    let mut out = String::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), s.read_to_string(&mut out)).await;
    let status = out.lines().next().unwrap_or_default().to_string();
    let body = out.split("\r\n\r\n").nth(1).unwrap_or_default().to_string();
    (status, body)
}

pub(crate) async fn get(addr: &str, host: &str) -> (String, String) {
    exchange(&mut TcpStream::connect(addr).await.unwrap(), host).await
}

pub(crate) async fn https_get(
    addr: &str,
    host: &str,
    root: Option<&[u8]>,
) -> Result<(String, String), String> {
    let mut roots = rustls::RootCertStore::empty();
    if let Some(r) = root {
        roots
            .add(rustls::pki_types::CertificateDer::from(r.to_vec()))
            .unwrap();
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let cfg = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let tcp = TcpStream::connect(addr).await.map_err(|e| e.to_string())?;
    let name = rustls::pki_types::ServerName::try_from(host.to_string()).unwrap();
    let mut tls = tokio_rustls::TlsConnector::from(Arc::new(cfg))
        .connect(name, tcp)
        .await
        .map_err(|e| e.to_string())?;
    Ok(exchange(&mut tls, host).await)
}

pub(crate) fn config(reg: &Registry, ca_dir: &Path) -> FrontConfig {
    let mut cfg = FrontConfig::for_zone(reg, ca_dir);
    cfg.plain = free();
    cfg.tls = free();
    cfg.ca_check_every = Duration::ZERO;
    cfg
}

pub(crate) async fn until(mut f: impl FnMut() -> bool) {
    for _ in 0..100 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("condition never held");
}
