//! Go's listen_test.go and heal_test.go, ported (each cites its Go name), on a network whose addresses are translated
//! like a Mac's but whose gateway is 127.0.0.1, so the registering half runs on any machine with no daemon.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use crate::front::{INGRESS_BIND, INGRESS_KEY, INGRESS_TLS_BIND, INGRESS_TLS_KEY};
use crate::nat::Forwards;
use crate::pool::{LOOPBACK, Net};
use crate::ports::{ports_path, read_ports};
use crate::registry::tests::home;
use crate::table::{TableCache, read_port_table};
use crate::{Error, Name, Registry};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::sync::Arc;

const TRANSLATED: Net = Net {
    a: 127,
    b: 0,
    virtual_: true,
};

fn reg(net: Net) -> Registry {
    let mut r = Registry::open(&home(), "doze");
    r.net = net;
    r
}

fn private(r: &Registry, ip: Ipv4Addr, public: u16) -> Option<u16> {
    read_port_table(r.path(), TRANSLATED).private(ip.octets(), public)
}

/// TestListenBindsTheAddressWhereItCan
#[test]
fn where_an_address_is_local_listen_binds_it() {
    let r = reg(LOOPBACK);
    let lease = r
        .claim_at(&Name::qualified("db", "shop"), Ipv4Addr::LOCALHOST)
        .unwrap();
    let ln = lease.listen(0).unwrap();
    assert_eq!(ln.addr().ip(), lease.ip);
    assert!(
        r.snapshot()["db.shop.doze"].ports.is_empty(),
        "a bound address registered ports"
    );
}

/// TestListenRegistersAPrivatePortWhereItCannot
#[test]
fn where_it_is_not_each_service_gets_a_private_port_the_table_has() {
    let r = reg(TRANSLATED);
    let mut held = Vec::new();
    for stack in ["shop", "blog"] {
        let lease = r.claim(&Name::qualified("db", stack)).unwrap();
        let ln = lease.listen(5432).unwrap();
        assert_eq!(
            ln.addr().to_string(),
            format!("{}:5432", lease.ip),
            "it answers for its public address"
        );
        held.push((lease, ln));
    }
    for (lease, ln) in &held {
        let p = private(&r, lease.ip, 5432).expect("in the table");
        let back = read_port_table(r.path(), TRANSLATED).public(lease.ip.octets(), p);
        assert_eq!(back, Some(5432));
        // The private port is the listener: what is sent there arrives.
        let mut c = TcpStream::connect((TRANSLATED.gateway(), p)).unwrap();
        c.write_all(lease.name.host.as_bytes()).unwrap();
        let (mut s, _) = ln.socket.accept().unwrap();
        let mut got = vec![0; lease.name.host.len()];
        s.read_exact(&mut got).unwrap();
        assert_eq!(got, lease.name.host.as_bytes());
    }
    // Claiming again, as doze does on every topology change, keeps them.
    let (first, _) = &held[0];
    r.claim_at(&first.name, first.ip).unwrap();
    assert!(
        r.snapshot()[&first.name.host].ports.contains_key("5432"),
        "re-claiming dropped its ports"
    );
    // Dropping a listener takes the registration with it.
    let host = first.name.host.clone();
    held.remove(0);
    assert!(
        r.snapshot()[&host].ports.is_empty(),
        "a closed listener is still registered"
    );
    assert!(
        !read_ports(r.path()).contains_key(&host),
        "nor in the ports file"
    );
}

/// TestARoutedNameIsServedByTheFrontDoor, and (new) the same for https on 443, which Go refused on macOS.
#[test]
fn a_routed_name_is_served_by_the_front_door_unless_it_took_the_port() {
    let r = reg(TRANSLATED);
    let api = r.claim(&Name::qualified("api", "shop")).unwrap();
    let aws = r.claim(&Name::qualified("aws", "shop")).unwrap();
    let db = r.claim(&Name::qualified("db", "shop")).unwrap();
    api.route("127.0.0.1:3000").unwrap();
    aws.route("127.0.0.1:4566").unwrap();
    aws.forward(80, 4566).unwrap();
    assert_eq!(
        private(&r, api.ip, 80),
        None,
        "port 80 forwarded with no front door running"
    );

    r.claim_holder(INGRESS_KEY, INGRESS_BIND).unwrap();
    assert_eq!(private(&r, api.ip, 80), Some(80));
    assert_eq!(private(&r, aws.ip, 80), Some(4566), "its own listener");
    assert_eq!(private(&r, db.ip, 80), None, "a name with no route");
    assert_eq!(private(&r, api.ip, 443), None, "no https front door yet");

    r.claim_holder(INGRESS_TLS_KEY, INGRESS_TLS_BIND).unwrap();
    assert_eq!(private(&r, api.ip, 443), Some(443));
    // A front door moved off the wildcard (a test's) is not reachable at the gateway: nothing is forwarded to it.
    r.claim_holder(INGRESS_KEY, "127.0.0.1:18080").unwrap();
    assert_eq!(private(&r, api.ip, 80), None);
}

/// TestRegistryListenFindsTheNameByItsAddress
#[test]
fn a_caller_with_only_an_address_gets_the_same_listener() {
    let r = reg(TRANSLATED);
    let lease = r.claim(&Name::qualified("db", "shop")).unwrap();
    let _ln = r.listen(SocketAddrV4::new(lease.ip, 5432)).unwrap();
    assert!(r.snapshot()["db.shop.doze"].ports.contains_key("5432"));
    let stranger = SocketAddrV4::new(TRANSLATED.at(99 << 8 | 9), 5432);
    assert!(matches!(r.listen(stranger), Err(Error::NotMine(_))));
    // An address outside the block is listened on as it is.
    r.listen(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
}

/// TestTheTableFollowsAServiceThatRestarted
#[test]
fn the_table_follows_a_service_that_restarted() {
    let r = reg(TRANSLATED);
    let lease = r.claim(&Name::qualified("db", "shop")).unwrap();
    let v = lease.ip.octets();
    lease.forward(5432, 51885).unwrap();
    let c = TableCache::new(r.path(), TRANSLATED);
    assert_eq!(c.current().private(v, 5432), Some(51885));
    // Same length, likely the same clock tick: only the file's identity says it changed.
    lease.forward(5432, 52094).unwrap();
    c.refresh(false);
    assert_eq!(c.current().private(v, 5432), Some(52094));
    assert_eq!(
        c.current().public(v, 51885),
        None,
        "the old private port still maps back"
    );
    let before = c.current();
    c.refresh(false);
    assert!(
        Arc::ptr_eq(&before, &c.current()),
        "rebuilt though nothing was written"
    );
}

/// TestAForwardLostToAnOlderWriterIsPutBack: the daemon does not need the registry's copy, so there is no heal.
#[test]
fn ports_stripped_by_an_older_writer_are_still_translated() {
    let r = reg(TRANSLATED);
    let lease = r.claim(&Name::qualified("db", "shop")).unwrap();
    let _ln = lease.listen(5432).unwrap();
    let want = r.snapshot()["db.shop.doze"].ports["5432"];
    let raw = std::fs::read_to_string(r.path()).unwrap();
    let mut m: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&raw).unwrap();
    for e in m.values_mut() {
        e.as_object_mut().unwrap().remove("ports");
    }
    std::fs::write(r.path(), serde_json::to_string(&m).unwrap()).unwrap();
    assert!(r.snapshot()["db.shop.doze"].ports.is_empty());
    assert_eq!(private(&r, lease.ip, 5432), Some(want));
}

/// TestReleaseDropsKeptPorts
#[test]
fn release_drops_the_ports_file_entry() {
    let r = reg(TRANSLATED);
    let lease = r.claim(&Name::qualified("db", "shop")).unwrap();
    lease.forward(5432, 51000).unwrap();
    assert!(read_ports(r.path()).contains_key("db.shop.doze"));
    lease.release().unwrap();
    assert!(!read_ports(r.path()).contains_key("db.shop.doze"));
}

/// TestKeptPortsFollowTheAddress
#[test]
fn ports_kept_for_one_address_are_not_used_at_another() {
    let r = reg(TRANSLATED);
    let lease = r.claim(&Name::qualified("db", "shop")).unwrap();
    lease.forward(5432, 51000).unwrap();
    let mut side = read_ports(r.path());
    let e = side.get_mut("db.shop.doze").unwrap();
    assert_eq!(e.ip, lease.ip.to_string());
    e.ip = "127.0.9.9".into();
    std::fs::write(ports_path(r.path()), serde_json::to_string(&side).unwrap()).unwrap();
    lease.unforward(5432).unwrap(); // the registry's own copy gone; only the side file could supply it
    side.get_mut("db.shop.doze")
        .unwrap()
        .ports
        .insert("5432".into(), 51000);
    std::fs::write(ports_path(r.path()), serde_json::to_string(&side).unwrap()).unwrap();
    assert_eq!(private(&r, lease.ip, 5432), None);
}

#[test]
fn a_port_of_zero_is_refused_and_a_name_not_held_cannot_be_forwarded() {
    let r = reg(TRANSLATED);
    let lease = r.claim(&Name::qualified("db", "shop")).unwrap();
    assert!(lease.forward(0, 51000).is_err());
    assert!(lease.forward(5432, 0).is_err());
    lease.release().unwrap();
    assert!(matches!(lease.forward(5432, 51000), Err(Error::NotHeld(_))));
}
