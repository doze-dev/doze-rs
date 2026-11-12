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

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[test]
fn a_stack_is_claimed_listened_routed_and_given_back() {
    let reg = Registry::open(&home(), "doze").with_net(TRANSLATED_LOOPBACK);
    let p = plan("shop", &[kafka(), aws("aws")], true).unwrap();
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
    let target = main.socket.local_addr().unwrap().to_string();
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
        .socket
        .local_addr()
        .unwrap()
        .to_string();
    let registry = bound.instances["events"].listeners["registry"]
        .socket
        .local_addr()
        .unwrap()
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

#[test]
fn without_names_each_listener_is_on_loopback() {
    let (a, b) = (free_port(), free_port());
    let p = plan(
        "shop",
        &[
            decl("db", Some(a), 0, vec![]),
            decl("cache", Some(b), 0, vec![]),
        ],
        false,
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
    let p = plan("shop", &[decl("db", Some(port), 0, vec![])], false).unwrap();
    let e = bind(&p, &Registry::open(&home(), "doze"), &sock).unwrap_err();
    assert!(e.to_string().starts_with("db: "), "{e}");
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
    };
    assert_eq!(
        e.to_string(),
        "db: db.shop.doze is already in use on this machine by doze (pid 42): stop it, or rename this stack or service"
    );
}
