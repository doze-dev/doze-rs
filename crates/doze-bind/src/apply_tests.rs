//! Binding against a private registry on a translated network whose daemon is 127.0.0.1, so it runs anywhere.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use super::*;
use crate::plan::plan;
use crate::plan::tests::{aws, decl, kafka};
use doze_names::pool::TRANSLATED_LOOPBACK;
use std::io::{Read, Write};
use std::net::TcpStream;

fn home() -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let i = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("doze-bind-{}-{i}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sock(inst: &str, l: &ListenerPlan) -> Address {
    Address::Unix(format!("/run/{inst}/{}.sock", l.name))
}

/// A loopback port nobody holds, below every system's ephemeral range (Linux starts at 32768, macOS and Windows at
/// 49152), and never the same one twice in this process: a port from `bind(":0")` can be handed straight back to a
/// test running alongside (Windows does), as doze-module-test's `free_port` found on Linux.
/// From 10000..20000: the engine hands out 20000..32000 from the same per-process offset, so a shared range let the
/// test's port and the engine's first one be the same (daemon_catalog on Linux, #110's queue run).
fn free_port() -> u16 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let start = std::process::id().wrapping_mul(97) % 10_000;
    loop {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let port = u16::try_from(10_000 + (start + n) % 10_000).unwrap();
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
}

#[test]
fn a_stack_is_claimed_listened_routed_and_given_back() {
    let reg = Registry::open(&home(), "doze").with_net(TRANSLATED_LOOPBACK);
    let p = plan("shop", &[kafka(), aws("aws")], Addressing::Own).unwrap();
    let bound = bind(&p, &reg, &sock).unwrap();

    let a = &bound.instances["aws"];
    assert_eq!(a.host, "aws.shop.doze");
    let main = &a.listeners["main"];
    assert_eq!(main.granted.url, "http://aws.shop.doze");
    assert_eq!(main.granted.hosts, ["aws.shop.doze", "sync-aws.shop.doze"]);
    assert_eq!(
        main.granted.backend,
        Address::Unix("/run/aws/main.sock".into())
    );
    // Routed to the name's address and public port, which the network daemon translates, not to the socket.
    let target = main.public.to_string();
    assert_eq!(
        target,
        reg.resolve("aws.shop.doze").unwrap().to_string() + ":4566"
    );
    assert_ne!(main.public, main.socket.local_addr().unwrap());
    for host in [
        "aws.shop.doze",
        "sync-aws.shop.doze",
        "s3.eu-west-1.aws.shop.doze",
    ] {
        assert_eq!(
            reg.front_target(host).as_deref(),
            Some(target.as_str()),
            "{host}"
        );
    }
    let k = &bound.instances["events"].grants();
    assert_eq!(k["main"].addr, "events.shop.doze:9092");
    assert_eq!(k["main"].url, "");
    let console = bound.instances["events"].listeners["console"]
        .public
        .to_string();
    let registry = bound.instances["events"].listeners["registry"]
        .public
        .to_string();
    assert_eq!(
        reg.front_target("events.shop.doze"),
        Some(console),
        "the console is the name itself"
    );
    assert_eq!(
        reg.front_target("x.registry.events.shop.doze"),
        Some(registry),
        "most specific wins"
    );

    // The socket is the public port: what is sent there arrives.
    let mut c = TcpStream::connect(main.socket.local_addr().unwrap()).unwrap();
    c.write_all(b"ping").unwrap();
    let (mut s, _) = main.socket.accept().unwrap();
    let mut got = [0; 4];
    s.read_exact(&mut got).unwrap();
    assert_eq!(&got, b"ping");

    drop(bound);
    let left: Vec<_> = reg
        .snapshot()
        .into_keys()
        .filter(|k| !k.starts_with('_'))
        .collect();
    assert!(left.is_empty(), "names left behind: {left:?}");
    assert!(
        doze_names::ports::read_ports(reg.path()).is_empty(),
        "ports left behind"
    );
}

/// A SYN from this machine to `to`, as the kernel hands it to the network daemon: from the daemon's own address.
fn syn(from: [u8; 4], to: SocketAddrV4) -> Vec<u8> {
    let mut p = vec![0u8; 40];
    p[0] = 0x45;
    p[2..4].copy_from_slice(&40u16.to_be_bytes());
    p[8] = 64;
    p[9] = 6;
    p[12..16].copy_from_slice(&from);
    p[16..20].copy_from_slice(&to.ip().octets());
    // A source port no listener is on, so it is not taken for a service's answer.
    p[20..22].copy_from_slice(&1u16.to_be_bytes());
    p[22..24].copy_from_slice(&to.port().to_be_bytes());
    p[32] = 5 << 4;
    p[33] = 0x02;
    p
}

/// Whoever holds the front door dials a route's target from this machine, so where addresses are translated the
/// packet goes to the network daemon, which must turn it into the listener's socket. Routed to the socket itself (the
/// daemon's own address and a private port) it was dropped, and `http://catalog.larder.doze` through :80 hung while
/// `:8002` answered.
#[test]
fn a_route_dialled_through_the_network_daemon_reaches_its_listener() {
    let reg = Registry::open(&home(), "doze").with_net(TRANSLATED_LOOPBACK);
    let p = plan("shop", &[kafka(), aws("aws")], Addressing::Own).unwrap();
    let bound = bind(&p, &reg, &sock).unwrap();
    let table = doze_names::table::read_port_table(reg.path(), TRANSLATED_LOOPBACK);
    let gw = TRANSLATED_LOOPBACK.gateway().octets();
    for (inst, listener, host) in [
        ("aws", "main", "aws.shop.doze"),
        ("aws", "main", "sync-aws.shop.doze"),
        ("events", "console", "events.shop.doze"),
        ("events", "registry", "x.registry.events.shop.doze"),
    ] {
        let target: SocketAddrV4 = reg.front_target(host).unwrap().parse().unwrap();
        let mut packet = syn(gw, target);
        assert!(
            doze_names::nat::translate(&mut packet, gw, &table),
            "{host}: the network daemon drops a dial to {target}"
        );
        let ip: [u8; 4] = packet[16..20].try_into().unwrap();
        let at = SocketAddrV4::new(ip.into(), u16::from_be_bytes([packet[22], packet[23]]));
        let socket = bound.instances[inst].listeners[listener]
            .socket
            .local_addr()
            .unwrap();
        assert_eq!(std::net::SocketAddr::V4(at), socket, "{host}");
    }
}

#[test]
fn without_names_each_listener_is_on_loopback() {
    let (a, b) = (free_port(), free_port());
    let p = plan(
        "shop",
        &[
            decl("db", Some(a), 0, vec![]),
            decl("cache", Some(b), 0, vec![]),
        ],
        Addressing::Off,
    )
    .unwrap();
    let reg = Registry::open(&home(), "doze");
    let bound = bind(&p, &reg, &sock).unwrap();
    let db = &bound.instances["db"];
    assert_eq!(
        (db.host.as_str(), db.listeners["main"].granted.addr.clone()),
        ("", format!("127.0.0.1:{a}"))
    );
    assert!(reg.snapshot().is_empty(), "claimed names with names off");
}

#[test]
fn a_port_already_taken_says_which_instance() {
    let taken = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    let p = plan(
        "shop",
        &[decl("db", Some(port), 0, vec![])],
        Addressing::Off,
    )
    .unwrap();
    let e = bind(&p, &Registry::open(&home(), "doze"), &sock).unwrap_err();
    let want = format!("db cannot listen on 127.0.0.1:{port}: ");
    assert!(e.to_string().starts_with(&want), "{e}");
}

#[test]
fn a_name_held_elsewhere_names_the_holder_and_the_fix() {
    let e = Error {
        instance: "db".into(),
        source: doze_names::Error::Held {
            host: "db.shop.doze".into(),
            pid: 42,
            owner: "doze".into(),
        },
        at: None,
    };
    assert_eq!(
        e.to_string(),
        "db: db.shop.doze is already in use on this machine by doze (pid 42): stop it, or rename this stack or service"
    );
}

#[test]
fn shared_addressing_claims_the_names_on_loopback_and_routes_them() {
    let reg = Registry::open(&home(), "doze");
    let (port, other) = (free_port(), free_port());
    let mut a = aws("aws");
    a.port = Some(port);
    let p = plan(
        "shop",
        &[a, decl("db", Some(other), 0, vec![])],
        Addressing::Shared,
    )
    .unwrap();
    let bound = bind(&p, &reg, &sock).unwrap();
    let aws = &bound.instances["aws"];
    let main = &aws.listeners["main"];
    assert_eq!(main.granted.addr, format!("aws.shop.doze:{port}"));
    assert_eq!(
        main.socket.local_addr().unwrap().to_string(),
        format!("127.0.0.1:{port}")
    );
    assert_eq!(reg.resolve("aws.shop.doze"), Some(crate::LOOPBACK));
    assert_eq!(reg.resolve("db.shop.doze"), Some(crate::LOOPBACK));
    assert_eq!(
        reg.front_target("sync-aws.shop.doze"),
        Some(format!("127.0.0.1:{port}"))
    );
    assert!(aws.lease.is_some(), "the lease is there for domains");
    drop(bound);
    assert!(
        reg.resolve("aws.shop.doze").is_none(),
        "released with the bound"
    );
}
#[cfg(unix)]
#[test]
fn a_route_name_another_program_holds_is_said_and_the_rest_is_bound() {
    let home = home();
    // Another program, alive, holding the sibling route's name.
    let mut other = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .unwrap();
    let file = serde_json::json!({
        "_format": {"format": 2},
        "sync-aws.shop.doze": {"ip": "127.0.0.40", "pid": other.id(), "owner": "doze-aws", "tier": "qualified"},
    });
    std::fs::write(home.join("names.json"), file.to_string()).unwrap();
    let reg = Registry::open(&home, "doze").with_net(TRANSLATED_LOOPBACK);
    let p = plan("shop", &[aws("aws")], Addressing::Own).unwrap();
    let bound = bind(&p, &reg, &sock).unwrap();
    let a = &bound.instances["aws"];
    let _ = other.kill();
    let _ = other.wait();
    assert_eq!(a.held.len(), 1, "{:?}", a.held);
    assert!(
        a.held[0].starts_with("sync-aws.shop.doze is held by doze-aws (pid "),
        "{}",
        a.held[0]
    );
    assert!(reg.front_target("aws.shop.doze").is_some());
}

#[test]
fn on_a_shared_address_a_routed_listener_whose_port_is_taken_is_reached_by_name() {
    let reg = Registry::open(&home(), "doze");
    let taken = TcpListener::bind("127.0.0.1:0").unwrap();
    let busy = taken.local_addr().unwrap().port();
    let mut k = kafka();
    k.port = Some(free_port());
    k.listeners[1].port = busy; // the registry's conventional port, another program's
    k.listeners[2].port = free_port(); // the console's: 9093 may be another program's on this machine too
    let p = plan("shop", &[k], Addressing::Shared).unwrap();
    let bound = bind(&p, &reg, &sock).unwrap();
    let e = &bound.instances["events"];
    let registry = &e.listeners["registry"];
    let at = registry.socket.local_addr().unwrap();
    assert_ne!(at.port(), busy);
    // The registry's port is the one this test took. The console's came from `free_port` and was free when asked
    // for, but another process may take it before the bind (queue run 45078190892: "console: ...:15275 is taken"),
    // and then it moves too, as it should. So: the registry moved, and anything else that moved says why.
    assert!(
        e.moved
            .iter()
            .any(|m| m.starts_with(&format!("registry: events.shop.doze:{busy} is taken"))),
        "{:?}",
        e.moved
    );
    assert!(
        e.moved
            .iter()
            .all(|m| m.contains(" is taken; it is reached by its name only")),
        "{:?}",
        e.moved
    );
    assert_eq!(
        reg.front_target("registry.events.shop.doze"),
        Some(at.to_string()),
        "the route goes where it listens"
    );
    // A port taken for the main listener, which is reached by port, is still an error.
    let mut k = kafka();
    k.port = Some(busy);
    let p = plan("shop", &[k], Addressing::Shared).unwrap();
    assert!(bind(&p, &reg, &sock).is_err());
}
