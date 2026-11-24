//! What the names front-door test shares: doze-aws started on a port of its own, free loopback ports, and an HTTP/1.1
//! client that dials the front door itself (plain or TLS, trusting one root), as a resolver pointing a name at it would.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

pub(crate) mod verify;

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

pub(crate) const REGION: &str = "us-east-1";

pub(crate) const BIN: &str = env!("CARGO_BIN_EXE_doze-aws");

/// A `provided` function: a loop over the Runtime API in sh that answers with the host its URL was called at.
pub(crate) const BOOTSTRAP: &[u8] = br#"#!/bin/sh
while true; do
  H=$(mktemp)
  EVENT=$(curl -sS -D "$H" "http://$AWS_LAMBDA_RUNTIME_API/2018-06-01/runtime/invocation/next")
  ID=$(grep -i '^Lambda-Runtime-Aws-Request-Id' "$H" | cut -d: -f2 | tr -d ' \r\n')
  rm -f "$H"
  curl -sS -X POST "http://$AWS_LAMBDA_RUNTIME_API/2018-06-01/runtime/invocation/$ID/response" -d "{\"from\":\"lambda\",\"event\":$EVENT}"
done
"#;

/// The server, stopped (and waited for) when dropped, failed test or not.
pub(crate) struct Server(pub(crate) Child);

impl Drop for Server {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            let _ = Command::new("kill").arg(self.0.id().to_string()).status();
        }
        #[cfg(not(unix))]
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Starts doze-aws in `dir` on a port of its choosing and reads the endpoint from its ready block.
pub(crate) fn start(dir: &Path) -> (Server, String) {
    std::fs::write(
        dir.join("doze-aws.toml"),
        "listen = \"127.0.0.1:0\"\ndata-dir = \"data\"\nconsole = \"off\"\n",
    )
    .unwrap();
    let mut child = Command::new(BIN)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .env_remove("AWS_ENDPOINT_URL")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut endpoint = None;
    for line in lines.by_ref() {
        let line = line.unwrap();
        if let Some(e) = line.trim().strip_prefix("endpoint") {
            endpoint = Some(e.trim().trim_start_matches("http://").to_owned());
        }
        if line.contains("when something looks wrong") {
            break;
        }
    }
    std::thread::spawn(move || lines.for_each(drop));
    let server = Server(child);
    (server, endpoint.expect("an endpoint in the ready block"))
}

/// A loopback port nobody holds, below every system's ephemeral range (as doze-names' front-door tests pick theirs).
pub(crate) fn free_port() -> u16 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let start = std::process::id().wrapping_mul(89) % 10_000;
    loop {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let port = u16::try_from(10_000 + (start + n) % 10_000).unwrap();
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
}

pub(crate) fn have(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Scheme {
    Http,
    Https,
}

/// Where the client sends everything, and what it trusts.
pub(crate) struct Door {
    pub(crate) plain: String,
    pub(crate) tls: String,
    pub(crate) root: Vec<u8>,
}

pub(crate) struct Answer {
    pub(crate) status: u16,
    pub(crate) head: String,
    pub(crate) body: String,
}

/// One request as an SDK under `--auth trust` signs it: routed by its credential scope, the signature taken on trust.
pub(crate) fn signed(service: &str) -> Vec<(String, String)> {
    vec![
        ("X-Amz-Date".into(), "20261124T000000Z".into()),
        ("X-Amz-Content-Sha256".into(), "UNSIGNED-PAYLOAD".into()),
        (
            "Authorization".into(),
            format!(
                "AWS4-HMAC-SHA256 Credential=test/20261124/{REGION}/{service}/aws4_request, \
                 SignedHeaders=host;x-amz-date, Signature=00"
            ),
        ),
    ]
}

pub(crate) async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
    s: &mut S,
    method: &str,
    host: &str,
    path: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Answer {
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes()).await.unwrap();
    s.write_all(body).await.unwrap();
    let mut raw = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(30), s.read_to_end(&mut raw)).await;
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, rest) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let chunked = head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked");
    let body = if chunked {
        dechunk(rest)
    } else {
        rest.to_owned()
    };
    Answer {
        status,
        head: head.to_owned(),
        body,
    }
}

pub(crate) fn dechunk(mut s: &str) -> String {
    let mut out = String::new();
    while let Some((size, rest)) = s.split_once("\r\n") {
        let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
        if n == 0 {
            break;
        }
        out.push_str(rest.get(..n).unwrap_or(rest));
        s = rest.get(n + 2..).unwrap_or("");
    }
    out
}

/// A request straight to doze-aws (the control plane: making the things the front door is then asked for).
pub(crate) async fn direct(
    endpoint: &str,
    method: &str,
    path: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Answer {
    let mut s = TcpStream::connect(endpoint).await.unwrap();
    exchange(&mut s, method, endpoint, path, headers, body).await
}

impl Door {
    /// A request for `host` through the front door; `Err` when the TLS handshake fails.
    pub(crate) async fn ask(
        &self,
        scheme: Scheme,
        method: &str,
        host: &str,
        path: &str,
        headers: &[(String, String)],
    ) -> Result<Answer, String> {
        match scheme {
            Scheme::Http => {
                let mut s = TcpStream::connect(&self.plain).await.unwrap();
                Ok(exchange(&mut s, method, host, path, headers, b"").await)
            }
            Scheme::Https => {
                let mut roots = rustls::RootCertStore::empty();
                roots
                    .add(rustls::pki_types::CertificateDer::from(self.root.clone()))
                    .unwrap();
                let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(
                    rustls::crypto::ring::default_provider(),
                ))
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_root_certificates(roots)
                .with_no_client_auth();
                let tcp = TcpStream::connect(&self.tls).await.unwrap();
                let name = rustls::pki_types::ServerName::try_from(host.to_owned()).unwrap();
                let mut s = tokio_rustls::TlsConnector::from(Arc::new(cfg))
                    .connect(name, tcp)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(exchange(&mut s, method, host, path, headers, b"").await)
            }
        }
    }

    pub(crate) async fn get(&self, scheme: Scheme, host: &str, path: &str) -> Answer {
        self.ask(scheme, "GET", host, path, &[])
            .await
            .unwrap_or_else(|e| panic!("{scheme:?} {host}{path}: {e}"))
    }
}

pub(crate) fn json(a: &Answer) -> serde_json::Value {
    serde_json::from_str(&a.body).unwrap_or_else(|e| panic!("{e}: {} {}", a.head, a.body))
}

/// A stored zip of one executable `bootstrap`.
pub(crate) fn zip_bootstrap() -> Vec<u8> {
    let (name, data) = ("bootstrap", BOOTSTRAP);
    let crc = crc32fast::hash(data);
    let (n, len) = (name.len() as u16, data.len() as u32);
    let mut out = Vec::new();
    out.extend_from_slice(&0x0403_4b50_u32.to_le_bytes());
    out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(data);
    let central_at = out.len() as u32;
    let mut central = Vec::new();
    central.extend_from_slice(&0x0201_4b50_u32.to_le_bytes());
    central.extend_from_slice(&[20, 3, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    central.extend_from_slice(&crc.to_le_bytes());
    central.extend_from_slice(&len.to_le_bytes());
    central.extend_from_slice(&len.to_le_bytes());
    central.extend_from_slice(&n.to_le_bytes());
    central.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
    central.extend_from_slice(&(0o100_755_u32 << 16).to_le_bytes());
    central.extend_from_slice(&0_u32.to_le_bytes());
    central.extend_from_slice(name.as_bytes());
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50_u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0, 1, 0, 1, 0]);
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&central_at.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out
}
