//! The wire filter on the real server (`DOZE_POSTGRES_BINDIR`): clients reach PostgreSQL only through the module's
//! `wire.accept`, as they do through doze. psql connects; TLS is terminated and required where the host says so; a
//! query is cancelled by psql's own CancelRequest (a connection of its own, with the synthetic key); a hand-written
//! driver sees a synthetic key, of the real one's length on protocol 3.2 (PostgreSQL 18), and cancels with it.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_module_host::{Backends, BoxFuture, Running};
use doze_module_proto::msg::console::WireAccept;
use doze_module_proto::msg::error::Error;
use doze_module_proto::msg::instance::Address;
use doze_module_test::{Booted, Harness, acceptance};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn harness() -> Option<Harness> {
    acceptance(env!("CARGO_BIN_EXE_doze-module-postgres"), "postgres").unwrap()
}

fn major(h: &Harness) -> u32 {
    let out = std::process::Command::new(h.bin_dir.join("postgres"))
        .arg("--version")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    text.split_whitespace()
        .last()
        .unwrap()
        .split('.')
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

/// The host's side: the instance is booted already; spliced sessions are counted.
struct Boots {
    addr: Address,
    in_use: AtomicI64,
}

impl Backends for Boots {
    fn boot<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<Address, Error>> {
        Box::pin(async move { Ok(self.addr.clone()) })
    }
    fn acquire(&self, _: &str) {
        self.in_use.fetch_add(1, Ordering::SeqCst);
    }
    fn release(&self, _: &str) {
        self.in_use.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A booted instance with a front door: a loopback port whose connections are handed to the module.
struct Front {
    booted: Booted,
    boots: Arc<Boots>,
    port: u16,
}

async fn front(h: &Harness, req: WireAccept) -> Front {
    let booted = h
        .boot(&format!(
            "postgres \"wire\" {{\n  version = {}\n}}\n",
            major(h)
        ))
        .await
        .unwrap();
    let boots = Arc::new(Boots {
        addr: booted.backend().unwrap(),
        in_use: AtomicI64::new(0),
    });
    let m: Arc<Running> = Arc::new(h.module_with(Some(boots.clone())).await.unwrap());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((c, _)) = l.accept().await {
            let (m, req) = (m.clone(), req.clone());
            let c = c.into_std().unwrap();
            tokio::spawn(async move { m.wire_accept(c, &req).await });
        }
    });
    Front {
        booted,
        boots,
        port,
    }
}

fn plain() -> WireAccept {
    WireAccept {
        instance: "wire".into(),
        require_tls: false,
        local: true,
        tls: None,
    }
}

fn psql(f: &Front, sslmode: &str, q: &str) -> std::process::Command {
    let mut c =
        std::process::Command::new(std::path::Path::new(&f.booted.toolchain.bin_dir).join("psql"));
    let conn = format!(
        "host=127.0.0.1 port={} user=postgres dbname=wire sslmode={sslmode}",
        f.port
    );
    c.args(["-X", "-tA", &conn, "-c", q]);
    c
}

fn run(mut c: std::process::Command) -> Result<String, String> {
    let out = c.output().unwrap();
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn psql_connects_through_the_filter_and_sessions_are_counted() {
    let Some(h) = harness() else { return };
    let f = front(&h, plain()).await;
    for _ in 0..3 {
        let got = tokio::task::spawn_blocking({
            let c = psql(&f, "prefer", "SELECT current_database() || ' ' || 1 + 1");
            move || run(c)
        });
        assert_eq!(got.await.unwrap().unwrap(), "wire 2");
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        f.boots.in_use.load(Ordering::SeqCst),
        0,
        "acquire and release do not balance"
    );
    f.booted.stop(Duration::from_secs(20)).await;
}

fn tls_files() -> (Vec<u8>, Vec<u8>) {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../kafka-server/tests/tls-fixtures/"
    );
    let cert = CertificateDer::from_pem_file(format!("{dir}ec-leaf-only.pem")).unwrap();
    let key = PrivateKeyDer::from_pem_file(format!("{dir}ec-key-pkcs8.pem")).unwrap();
    (cert.to_vec(), key.secret_der().to_vec())
}

#[tokio::test(flavor = "multi_thread")]
async fn tls_is_terminated_and_required_where_the_host_says() {
    let Some(h) = harness() else { return };
    let (cert, key) = tls_files();
    let req = WireAccept {
        require_tls: true,
        local: false,
        tls: Some((cert.into(), key.into())),
        ..plain()
    };
    let f = front(&h, req).await;
    let refused = tokio::task::spawn_blocking({
        let c = psql(&f, "disable", "SELECT 1");
        move || run(c)
    });
    let e = refused.await.unwrap().unwrap_err();
    assert!(e.contains("SSL connection required"), "{e}");
    let ok = tokio::task::spawn_blocking({
        let c = psql(&f, "require", "SELECT 1");
        move || run(c)
    });
    assert_eq!(ok.await.unwrap().unwrap(), "1");
    f.booted.stop(Duration::from_secs(20)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn psql_cancels_a_query_through_the_filter() {
    let Some(h) = harness() else { return };
    let f = front(&h, plain()).await;
    let mut slow = tokio::process::Command::from(psql(&f, "disable", "SELECT pg_sleep(60)"));
    let child = slow
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let pid = child.id().unwrap();
    // Wait until the query runs on the server, then interrupt psql: it sends a CancelRequest of its own.
    wait_for_sleep(&f.booted).await;
    let killed = std::process::Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let out = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("canceling statement due to user request"),
        "{err}"
    );
    f.booted.stop(Duration::from_secs(20)).await;
}

/// Polls the server (on its socket) until a pg_sleep query runs.
async fn wait_for_sleep(b: &Booted) {
    let Some(Address::Unix(sock)) = b.backend() else {
        panic!()
    };
    let p = std::path::Path::new(&sock);
    let dir = p.parent().unwrap().to_string_lossy().to_string();
    let port = p
        .file_name()
        .unwrap()
        .to_string_lossy()
        .trim_start_matches(".s.PGSQL.")
        .to_string();
    let q = "SELECT count(*) FROM pg_stat_activity WHERE query LIKE 'SELECT pg_sleep%' AND state = 'active'";
    for _ in 0..100 {
        let mut c =
            std::process::Command::new(std::path::Path::new(&b.toolchain.bin_dir).join("psql"));
        c.args([
            "-X", "-tA", "-h", &dir, "-p", &port, "-U", "postgres", "-d", "wire", "-c", q,
        ]);
        if run(c).as_deref() == Ok("1") {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("the query never ran");
}

/// A hand-written driver's messages.
async fn send_startup(s: &mut tokio::net::TcpStream, minor: u32) {
    let mut body = (3u32 << 16 | minor).to_be_bytes().to_vec();
    body.extend(b"user\0postgres\0database\0wire\0\0");
    let len = u32::try_from(body.len() + 4).unwrap();
    s.write_all(&len.to_be_bytes()).await.unwrap();
    s.write_all(&body).await.unwrap();
}

/// Messages until ReadyForQuery: (type, payload) each.
async fn until_ready(s: &mut tokio::net::TcpStream) -> Vec<(u8, Vec<u8>)> {
    let mut out = Vec::new();
    loop {
        let mut h = [0u8; 5];
        s.read_exact(&mut h).await.unwrap();
        let len = u32::from_be_bytes([h[1], h[2], h[3], h[4]]) as usize - 4;
        let mut p = vec![0u8; len];
        s.read_exact(&mut p).await.unwrap();
        out.push((h[0], p));
        if h[0] == b'Z' {
            return out;
        }
    }
}

async fn query(s: &mut tokio::net::TcpStream, q: &str) -> Vec<(u8, Vec<u8>)> {
    let len = u32::try_from(q.len() + 5).unwrap();
    let mut m = vec![b'Q'];
    m.extend(len.to_be_bytes());
    m.extend(q.as_bytes());
    m.push(0);
    s.write_all(&m).await.unwrap();
    until_ready(s).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_driver_sees_a_synthetic_key_and_cancels_with_it() {
    let Some(h) = harness() else { return };
    let f = front(&h, plain()).await;
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", f.port))
        .await
        .unwrap();
    // Protocol 3.2: PostgreSQL 18 answers with a 32-byte secret; older ones negotiate down to 3.0 and 4 bytes.
    send_startup(&mut s, 2).await;
    let hello = until_ready(&mut s).await;
    let key = hello
        .iter()
        .find(|(t, _)| *t == b'K')
        .map(|(_, p)| p.clone())
        .unwrap();
    let want = if major(&h) >= 18 { 4 + 32 } else { 4 + 4 };
    assert_eq!(key.len(), want, "{hello:?}");
    let rows = query(&mut s, "SELECT pg_backend_pid()").await;
    let row = rows
        .iter()
        .find(|(t, _)| *t == b'D')
        .map(|(_, p)| p.clone())
        .unwrap();
    let real: u32 = String::from_utf8_lossy(&row[6..]).parse().unwrap();
    assert_ne!(
        u32::from_be_bytes([key[0], key[1], key[2], key[3]]),
        real,
        "the real key reached the client"
    );
    // A long query, cancelled from a connection of its own with the synthetic key.
    let sql = b"SELECT pg_sleep(60)\0";
    let mut m = vec![b'Q'];
    m.extend(u32::try_from(sql.len() + 4).unwrap().to_be_bytes());
    m.extend(sql);
    s.write_all(&m).await.unwrap();
    wait_for_sleep(&f.booted).await;
    let mut c = tokio::net::TcpStream::connect(("127.0.0.1", f.port))
        .await
        .unwrap();
    let mut cancel = u32::try_from(8 + key.len()).unwrap().to_be_bytes().to_vec();
    cancel.extend(80_877_102u32.to_be_bytes());
    cancel.extend(&key);
    c.write_all(&cancel).await.unwrap();
    let answer = tokio::time::timeout(Duration::from_secs(10), until_ready(&mut s))
        .await
        .unwrap();
    let error = answer
        .iter()
        .find(|(t, _)| *t == b'E')
        .map(|(_, p)| String::from_utf8_lossy(p).to_string());
    assert!(
        error.as_deref().is_some_and(|e| e.contains("57014")),
        "{answer:?}"
    );
    f.booted.stop(Duration::from_secs(20)).await;
}
