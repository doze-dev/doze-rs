//! Go's netd_test.go, ported, and the daemon's two small parsers.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use super::*;
use crate::Name;
use crate::pool::LOOPBACK;
use crate::registry::FILE_NAME;
use crate::registry::tests::home;
use crate::resolver::free_udp_addr;
use crate::upstream::Upstreams;
use crate::wire::TYPE_A;
use crate::wire::tests::{ancount, query, rcode};
use std::net::SocketAddr;
use tokio::net::UdpSocket;

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

/// TestTheNetworkDaemonAnswersTheZoneWhenNoProgramDoes (the forwarding half is the resolver's own test).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_daemon_answers_the_zone_when_no_program_does() {
    let (h, addr) = (home(), free_udp_addr().unwrap());
    let mut cfg = zone_config(&h.join(FILE_NAME));
    cfg.addr = addr.to_string();
    cfg.upstreams = Arc::new(Upstreams::fixed(vec![]));
    let mut netd = serve(cfg);
    assert!(
        netd.bound(Duration::from_secs(3)).await,
        "the daemon never took the resolver"
    );
    assert_eq!(
        rcode(&ask(addr, "web.shop.doze").await),
        3,
        "nothing runs: NXDOMAIN"
    );
    let mut r = Registry::open(&h, "doze");
    r.net = LOOPBACK;
    assert_eq!(r.resolver_holder().map(|e| e.owner).as_deref(), Some(OWNER));

    // A stack comes up: answered from the registry at once, nothing restarted.
    let web = r.claim(&Name::qualified("web", "shop")).unwrap();
    let resp = ask(addr, "web.shop.doze").await;
    assert_eq!(ancount(&resp), 1);
    assert_eq!(&resp[resp.len() - 4..], &web.ip.octets());

    // The program's own resolver stands by behind the daemon and takes over when it goes.
    let mut peer_cfg = ResolverConfig::from_registry(&r);
    peer_cfg.addr = addr.to_string();
    peer_cfg.upstreams = Arc::new(Upstreams::fixed(vec![]));
    let mut peer = serve(peer_cfg);
    assert!(
        !peer.bound(Duration::from_millis(200)).await,
        "a program took the resolver from the daemon"
    );
    netd.close().await;
    assert!(
        peer.bound(Duration::from_secs(5)).await,
        "the program never took over"
    );
    peer.close().await;
}

#[test]
fn a_connection_opens_on_a_bare_syn() {
    let mut p = vec![0u8; 40];
    p[0] = 0x45;
    p[9] = 6;
    p[33] = 0x02;
    assert!(first_packet(&p));
    p[33] = 0x12; // SYN+ACK: the answer, not an opening
    assert!(!first_packet(&p));
    p[33] = 0x02;
    p[9] = 17;
    assert!(!first_packet(&p), "not TCP");
    assert!(!first_packet(&p[..20]), "too short");
    assert!(!first_packet(&[]));
}

#[test]
fn the_block_is_free_unless_something_more_specific_routes_it() {
    let default = "   route to: 198.19.0.10\ndestination: default\n       mask: default\n    gateway: 192.168.1.1\n  interface: en0\n";
    assert_eq!(route_owner(default), None);
    let taken = "   route to: 198.19.0.10\ndestination: 198.19.0.0\n       mask: 255.255.128.0\n  interface: utun7\n";
    assert_eq!(route_owner(taken).as_deref(), Some("198.19.0.0 on utun7"));
    assert_eq!(
        route_owner(""),
        None,
        "route failed: nothing to say it is taken"
    );
}
