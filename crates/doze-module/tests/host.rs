//! The host crate against the real echo module: the pool, typed calls, the host's downloads, a client handed over and
//! spliced to a backend the host boots, notices, a module relaunched after it dies, and idle modules stopped.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_module_host::{Backends, BoxFuture, Downloads, Notice, Pool, Services};
use doze_module_proto::msg::console::WireAccept;
use doze_module_proto::msg::error::Error;
use doze_module_proto::msg::instance::{Address, Instance, Toolchain};
use doze_module_proto::msg::run::{Ensure, Ensured, ResolveArgs, WithToolchain};
use doze_module_proto::msg::value::{Node, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

struct Mirror;
impl Downloads for Mirror {
    fn resolve_major<'a>(
        &'a self,
        _e: &'a str,
        _m: &'a str,
    ) -> BoxFuture<'a, Result<String, Error>> {
        Box::pin(async { Ok("2.4.1".to_string()) })
    }
    fn ensure<'a>(&'a self, r: &'a Ensure) -> BoxFuture<'a, Result<Ensured, Error>> {
        Box::pin(async move {
            Ok(Ensured {
                bin_dir: format!("/cache/{}/bin", r.full),
                digest: "sha256:abc".into(),
            })
        })
    }
}

/// A backend that answers READY to a startup line and echoes the rest; counts boots and in-use connections.
struct Backend {
    addr: String,
    boots: AtomicU32,
    in_use: AtomicI64,
}
impl Backends for Backend {
    fn boot<'a>(&'a self, _i: &'a str) -> BoxFuture<'a, Result<Address, Error>> {
        self.boots.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(Address::Tcp(self.addr.clone())) })
    }
    fn acquire(&self, _i: &str) {
        self.in_use.fetch_add(1, Ordering::SeqCst);
    }
    fn release(&self, _i: &str) {
        self.in_use.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn backend() -> Arc<Backend> {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap().to_string();
    tokio::spawn(async move {
        while let Ok((s, _)) = l.accept().await {
            tokio::spawn(async move {
                let mut s = BufReader::new(s);
                let mut line = String::new();
                if s.read_line(&mut line).await.is_ok() && line.starts_with("STARTUP") {
                    let mut s = s.into_inner();
                    let _ = s.write_all(b"READY\n").await;
                    let (mut r, mut w) = s.split();
                    let _ = tokio::io::copy(&mut r, &mut w).await;
                }
            });
        }
    });
    Arc::new(Backend {
        addr,
        boots: AtomicU32::new(0),
        in_use: AtomicI64::new(0),
    })
}

fn pool(b: Option<Arc<Backend>>, notices: Arc<Mutex<Vec<String>>>, idle: Duration) -> Pool {
    let sink: doze_module_host::Notices = Arc::new(move |engine, n| {
        if let Notice::Log(r) = n {
            notices
                .lock()
                .unwrap()
                .push(format!("{engine}: {}", r.message));
        }
    });
    let services = Services {
        downloads: Arc::new(Mirror),
        backends: b.map(|b| b as Arc<dyn Backends>),
        notices: Some(sink),
    };
    let locate: doze_module_host::Locate =
        Arc::new(|e| (e == "echo").then(|| env!("CARGO_BIN_EXE_doze-module-echo").into()));
    Pool::new(services, locate, idle)
}

fn instance() -> Instance {
    Instance {
        name: "e".into(),
        engine: "echo".into(),
        version: "2".into(),
        data_dir: String::new(),
        socket_dir: "/run/e".into(),
        spec: Node::bare(Value::Map(BTreeMap::new())),
        host: String::new(),
        listeners: BTreeMap::new(),
        deps: vec![],
        toolchain: None,
        dir: String::new(),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn typed_calls_downloads_and_notices_through_the_pool() {
    let notes = Arc::new(Mutex::new(Vec::new()));
    let p = pool(None, notes.clone(), Duration::from_secs(60));
    let m = p.get("echo").await.unwrap();
    assert_eq!(m.manifest.engine, "echo");
    let r = m
        .resolve(&ResolveArgs {
            instance: instance(),
            target: "t".into(),
            pins: vec![],
        })
        .await
        .unwrap();
    assert_eq!(r.toolchain.bin_dir, "/cache/2.4.1/bin");
    let tc = Toolchain {
        engine: "echo".into(),
        full: "2.4.1".into(),
        bin_dir: "/x".into(),
        tools: BTreeMap::new(),
    };
    m.plan(&WithToolchain {
        instance: instance(),
        toolchain: tc,
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        notes
            .lock()
            .unwrap()
            .contains(&"echo: planning the echo engine".to_string()),
        "{:?}",
        notes.lock().unwrap()
    );
    assert!(p.get("nope").await.is_err());
}

/// A client handed to the module, which reads its startup line, has the host boot the backend, and splices.
#[tokio::test(flavor = "current_thread")]
async fn a_client_is_handed_over_spliced_and_accounted() {
    let b = backend().await;
    let p = pool(Some(b.clone()), Arc::default(), Duration::from_secs(60));
    let m = p.get("echo").await.unwrap();
    let front = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    for i in 0..20 {
        let mut client = tokio::net::TcpStream::connect(front.local_addr().unwrap())
            .await
            .unwrap();
        let (accepted, _) = front.accept().unwrap();
        let req = WireAccept {
            instance: "e".into(),
            require_tls: false,
            local: true,
            tls: None,
        };
        let talk = async {
            client.write_all(b"STARTUP\n").await.unwrap();
            let mut ready = [0u8; 6];
            client.read_exact(&mut ready).await.unwrap();
            assert_eq!(&ready, b"READY\n");
            let token = format!("ping-{i}");
            client.write_all(token.as_bytes()).await.unwrap();
            let mut back = vec![0u8; token.len()];
            client.read_exact(&mut back).await.unwrap();
            assert_eq!(back, token.as_bytes());
            client.shutdown().await.unwrap();
        };
        let (r, ()) = tokio::join!(m.wire_accept(accepted, &req), talk);
        r.unwrap();
    }
    assert_eq!(b.boots.load(Ordering::SeqCst), 20);
    assert_eq!(
        b.in_use.load(Ordering::SeqCst),
        0,
        "acquire and release do not balance"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_dead_module_is_started_again_and_an_idle_one_stopped() {
    let p = pool(None, Arc::default(), Duration::from_millis(200));
    let first = p.get("echo").await.unwrap();
    first.shutdown(Duration::from_secs(2)).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!first.is_alive());
    let second = p.get("echo").await.unwrap();
    assert!(
        !Arc::ptr_eq(&first, &second) && second.is_alive(),
        "the dead module was not replaced"
    );
    second.network(&instance()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(p.reap_idle().await, vec!["echo".to_string()]);
    assert!(p.running().await.is_empty());
}
