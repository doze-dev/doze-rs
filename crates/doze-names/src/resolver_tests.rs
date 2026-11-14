//! Go's resolver tests, ported: one peer serves every peer's names, a peer takes over when the holder exits, and a
//! domain nobody declares is forwarded upstream while the zone never is. Each runs on a free loopback port and a
//! private home, so it never meets a real doze on this machine.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use super::*;
use crate::Name;
use crate::pool::LOOPBACK;
use crate::wire::TYPE_A;
use crate::wire::tests::{ancount, query, rcode};

fn home() -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let i = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("doze-names-resolver-{}-{i}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn reg(home: &std::path::Path, owner: &str) -> Registry {
    let mut r = Registry::open(home, owner);
    r.net = LOOPBACK;
    r
}

fn config(r: &Registry, addr: SocketAddr, upstreams: Upstreams) -> ResolverConfig {
    let mut cfg = ResolverConfig::from_registry(r);
    let resolving = r.clone();
    cfg.resolve = Arc::new(move |h| resolving.resolve(h));
    cfg.addr = addr.to_string();
    cfg.upstreams = Arc::new(upstreams);
    cfg
}

async fn ask(addr: SocketAddr, host: &str) -> Vec<u8> {
    let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    sock.connect(addr).await.unwrap();
    sock.send(&query(host, TYPE_A)).await.unwrap();
    let mut buf = vec![0u8; 512];
    let n = tokio::time::timeout(Duration::from_secs(2), sock.recv(&mut buf))
        .await
        .expect("no answer")
        .unwrap();
    buf.truncate(n);
    buf
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_peer_serves_every_peers_names() {
    // Go: TestOnePeerServesEveryPeersNames. Whichever program is installed powers the zone for all of them.
    let (h, addr) = (home(), free_udp_addr().unwrap());
    let kafka = reg(&h, "doze-kafka");
    kafka.claim(&Name::apex("kafka")).unwrap();
    let mut holder = serve(config(&kafka, addr, Upstreams::fixed(vec![])));
    assert!(holder.bound(RETRY * 3).await, "the first peer never bound");
    assert_eq!(
        kafka.resolver_holder().map(|e| e.owner).as_deref(),
        Some("doze-kafka")
    );
    let aws = reg(&h, "doze-aws");
    aws.claim(&Name::apex("aws")).unwrap();
    let mut standby = serve(config(&aws, addr, Upstreams::fixed(vec![])));
    assert!(
        !standby.bound(Duration::from_millis(200)).await,
        "the second peer took a socket a peer held"
    );
    for (host, want) in [("kafka.doze", [127, 0, 0, 3]), ("aws.doze", [127, 0, 0, 2])] {
        let resp = ask(addr, host).await;
        assert_eq!(ancount(&resp), 1, "{host}");
        assert_eq!(&resp[resp.len() - 4..], &want, "{host}");
    }
    standby.close().await;
    holder.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_peer_takes_over_when_the_holder_exits() {
    // Go: TestPeerTakesOverWhenTheHolderExits. Without the retry loop the zone would stay dark.
    let (h, addr) = (home(), free_udp_addr().unwrap());
    let kafka = reg(&h, "doze-kafka");
    kafka.claim(&Name::apex("kafka")).unwrap();
    let mut holder = serve(config(&kafka, addr, Upstreams::fixed(vec![])));
    assert!(holder.bound(RETRY * 3).await);
    let aws = reg(&h, "doze-aws");
    aws.claim(&Name::apex("aws")).unwrap();
    let mut standby = serve(config(&aws, addr, Upstreams::fixed(vec![])));
    holder.close().await;
    assert!(
        standby.bound(RETRY * 3).await,
        "the standby never took the socket over"
    );
    assert_eq!(ancount(&ask(addr, "aws.doze").await), 1);
    assert_eq!(
        aws.resolver_holder().map(|e| e.owner).as_deref(),
        Some("doze-aws")
    );
    standby.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_holder_letting_go_leaves_its_successors_record() {
    // The crossing behind the take-over test's failures on windows-11-arm, made to happen every time instead of once
    // in a few hundred runs: the successor's record is written while the holder still serves (as when a peer binds
    // in the instant the holder's socket closes), then the holder lets go. Both peers have this process's pid, so
    // only the record's owner and address say whose it is; released by pid alone, the successor's record went.
    let (h, addr) = (home(), free_udp_addr().unwrap());
    let kafka = reg(&h, "doze-kafka");
    let mut holder = serve(config(&kafka, addr, Upstreams::fixed(vec![])));
    assert!(holder.bound(RETRY * 3).await);
    let aws = reg(&h, "doze-aws");
    aws.claim_holder(RESOLVER_KEY, &addr.to_string()).unwrap();
    holder.close().await;
    assert_eq!(
        aws.resolver_holder().map(|e| e.owner).as_deref(),
        Some("doze-aws"),
        "the holder's release erased its successor's record"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_holder_that_exits_takes_its_record_with_it() {
    let (h, addr) = (home(), free_udp_addr().unwrap());
    let kafka = reg(&h, "doze-kafka");
    let mut holder = serve(config(&kafka, addr, Upstreams::fixed(vec![])));
    assert!(holder.bound(RETRY * 3).await);
    assert_eq!(
        kafka.resolver_holder().map(|e| e.owner).as_deref(),
        Some("doze-kafka")
    );
    holder.close().await;
    assert_eq!(
        kafka.resolver_holder(),
        None,
        "a holder that left is still named"
    );
}

/// A stand-in upstream: answers every query with one A record (TTL an hour).
async fn upstream_server() -> SocketAddr {
    let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = sock.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 512];
        while let Ok((n, peer)) = sock.recv_from(&mut buf).await {
            let any: &dyn Fn(&str) -> Option<Ipv4Addr> = &|_| Some(Ipv4Addr::new(93, 184, 216, 34));
            let Some((mut resp, _)) = answer(&buf[..n], any) else {
                continue;
            };
            resp[2] &= 0xF0; // NOERROR
            let ttl_at = resp.len() - 10;
            resp[ttl_at..ttl_at + 4].copy_from_slice(&3600u32.to_be_bytes());
            let _ = sock.send_to(&resp, peer).await;
        }
    });
    addr
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unserved_domain_is_forwarded_upstream_and_the_zone_never_is() {
    // Go: TestAnUnservedDomainIsForwardedUpstreamAndTheZoneNeverIs.
    let (h, addr) = (home(), free_udp_addr().unwrap());
    let r = reg(&h, "doze");
    let up = upstream_server().await;
    let mut s = serve(config(&r, addr, Upstreams::fixed(vec![up])));
    assert!(s.bound(RETRY * 3).await);
    let resp = ask(addr, "dev.acme.com").await;
    assert_eq!(
        (rcode(&resp), ancount(&resp)),
        (0, 1),
        "the internet's answer, relayed"
    );
    assert_eq!(&resp[resp.len() - 4..], &[93, 184, 216, 34]);
    let ttl_at = resp.len() - 10;
    assert_eq!(
        &resp[ttl_at..ttl_at + 4],
        &10u32.to_be_bytes(),
        "cached ten seconds at most"
    );
    let resp = ask(addr, "typo.doze").await;
    assert_eq!(
        rcode(&resp),
        3,
        "a typo in the zone is NXDOMAIN, never forwarded"
    );
    s.close().await;
}
