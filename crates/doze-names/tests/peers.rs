//! The zone is shared by programs built and released separately, so this runs them as what they are: separate
//! processes with one home. Each peer is this test binary run again with `DOZE_NAMES_TEST_PEER` set: it claims its
//! names, routes them to a server that says who it is, joins the resolver and the front door, and runs until its stdin
//! closes. Go: peers_test.go.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_names::front_door::FrontConfig;
use doze_names::resolver::ResolverConfig;
use doze_names::{Name, Registry};
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const PEER: &str = "DOZE_NAMES_TEST_PEER";
const NAMES: &str = "DOZE_NAMES_TEST_PEER_NAMES";

/// The peer's whole life, when this binary was started as one; otherwise nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn peer_main() {
    let Ok(owner) = std::env::var(PEER) else {
        return;
    };
    let home = std::path::PathBuf::from(std::env::var("DOZE_HOME").unwrap());
    let (dns, front) = (
        std::env::var("DOZE_ZONE_RESOLVER").unwrap(),
        std::env::var("DOZE_ZONE_INGRESS").unwrap(),
    );
    let srv = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = srv.local_addr().unwrap().to_string();
    let me = owner.clone();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = srv.accept().await {
            let mut buf = [0u8; 2048];
            let _ = s.read(&mut buf).await;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{me}",
                me.len()
            );
            let _ = s.write_all(resp.as_bytes()).await;
        }
    });
    let reg = Registry::open(&home, &owner);
    let names = std::env::var(NAMES).unwrap();
    let leases: Vec<_> = names
        .split(',')
        .map(|n| {
            let (svc, stack) = n.split_once('.').unwrap();
            let l = reg.claim(&Name::qualified(svc, stack)).unwrap();
            l.route(&target).unwrap();
            l
        })
        .collect();
    let mut dns_cfg = ResolverConfig::from_registry(&reg);
    dns_cfg.addr = dns;
    let _dns = doze_names::resolver::serve(dns_cfg);
    let mut front_cfg = FrontConfig::for_zone(&reg, &home.join("ca"));
    front_cfg.plain = front;
    front_cfg.tls = "127.0.0.1:0".into();
    let _front = doze_names::front_door::serve(front_cfg);
    println!("ready");
    let _ = tokio::task::spawn_blocking(|| {
        std::io::Read::read_to_end(&mut std::io::stdin(), &mut Vec::new())
    })
    .await;
    drop(leases);
}

struct Peer(Child);

impl Peer {
    fn start(home: &std::path::Path, owner: &str, names: &str, dns: &str, front: &str) -> Peer {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "peer_main", "--nocapture", "--test-threads=1"])
            .env(PEER, owner)
            .env(NAMES, names)
            .env("DOZE_HOME", home)
            .env("DOZE_ZONE_RESOLVER", dns)
            .env("DOZE_ZONE_INGRESS", front)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        while !line.contains("ready") {
            line.clear();
            assert!(
                out.read_line(&mut line).unwrap() > 0,
                "peer {owner} exited before ready"
            );
        }
        Peer(child)
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn get(front: &str, host: &str) -> String {
    let Ok(mut s) = tokio::net::TcpStream::connect(front).await else {
        return String::new();
    };
    let req = format!("GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).await.unwrap();
    let mut out = String::new();
    let _ = tokio::time::timeout(Duration::from_secs(3), s.read_to_string(&mut out)).await;
    out.split("\r\n\r\n").nth(1).unwrap_or_default().to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn separate_programs_share_the_zone_and_survive_one_dying() {
    if std::env::var(PEER).is_ok() {
        return;
    }
    let home = std::env::temp_dir().join(format!("doze-names-peers-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    let dns = doze_names::resolver::free_udp_addr().unwrap().to_string();
    let front = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string();
    let aws = Peer::start(&home, "doze-aws", "aws.harbour", &dns, &front);
    let _kafka = Peer::start(&home, "doze-kafka", "kafka.harbour", &dns, &front);
    // Whichever holds the front door, each name reaches its own program.
    assert_eq!(get(&front, "aws.harbour.doze").await, "doze-aws");
    assert_eq!(get(&front, "kafka.harbour.doze").await, "doze-kafka");
    let reg = Registry::open(&home, "observer");
    let holder = reg.ingress_holder().unwrap().owner;
    // The holder dies without a word (kill -9); the other takes the door over and its name still works.
    let survivor = if holder == "doze-aws" {
        drop(aws);
        "kafka.harbour.doze"
    } else {
        drop(_kafka);
        "aws.harbour.doze"
    };
    let mut answered = String::new();
    for _ in 0..60 {
        answered = get(&front, survivor).await;
        if !answered.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        !answered.is_empty(),
        "the surviving program never took the front door over"
    );
    assert_ne!(
        reg.ingress_holder().map(|e| e.owner),
        Some(holder),
        "the dead holder was pruned"
    );
}
