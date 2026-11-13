//! The door to other machines in the binary (SEC-250): a console on every interface in front of an open broker answers another
//! machine (here: this machine's own network address, which the console cannot tell from another's) until an ACL is made over
//! Kafka, then refuses it while loopback is still answered, and answers it again once every ACL is deleted. The log says each
//! change once.

use kafka_protocol::messages::create_acls_request::AclCreation;
use kafka_protocol::messages::delete_acls_request::DeleteAclsFilter;
use kafka_protocol::messages::{
    CreateAclsRequest, CreateAclsResponse, DeleteAclsRequest, DeleteAclsResponse,
};

use super::console::{console_addr, http};
use super::*;

/// This machine's address on its network (the one a packet to the outside would leave from), or `None` where it has none: no
/// packet is sent.
fn network_address() -> Option<std::net::IpAddr> {
    let probe = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    probe.connect("192.0.2.1:9").ok()?;
    let ip = probe.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

/// `GET /api/v1/home` at `host` (`ip:port`): the status line and the body.
fn home(host: &str) -> String {
    http(
        host,
        &format!("GET /api/v1/home HTTP/1.1\r\nHost: {host}\r\nX-Console: 1"),
        "",
    )
}

/// Waits until the broker's log holds `needle`.
fn logged(broker: &Broker, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let log = broker.log.lock().unwrap().clone();
        if log.contains(needle) {
            return log;
        }
        assert!(Instant::now() < deadline, "never logged {needle:?}: {log}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn an_acl_made_over_kafka_closes_a_console_on_the_network_to_other_machines_until_none_is_left() {
    let Some(ip) = network_address() else {
        eprintln!("skipped: this machine has no network address other than loopback");
        return;
    };
    let data = scratch("console-door");
    let mut broker = start_with(
        &data,
        &["--listen", "0.0.0.0:0", "--console-addr", "0.0.0.0:0"],
    );
    let port = console_addr(&broker).rsplit(':').next().unwrap().to_owned();
    let other = if ip.is_ipv6() {
        format!("[{ip}]:{port}")
    } else {
        format!("{ip}:{port}")
    };
    let local = format!("127.0.0.1:{port}");
    if std::net::TcpStream::connect_timeout(&other.parse().unwrap(), Duration::from_secs(2))
        .is_err()
    {
        eprintln!("skipped: the console cannot be reached at {other} (a firewall?)");
        assert!(broker.terminate().success());
        return;
    }
    let open = home(&other);
    assert!(open.starts_with("HTTP/1.1 200"), "{open}");
    // An ACL over Kafka; once it is answered, the network address is refused and loopback is not.
    let mut c = broker.connect();
    let mut acl = AclCreation::default();
    acl.resource_type = 2;
    acl.resource_name = StrBytes::from_static_str("elsewhere");
    acl.resource_pattern_type = 3;
    acl.principal = StrBytes::from_static_str("User:someone");
    acl.host = StrBytes::from_static_str("*");
    acl.operation = 8;
    acl.permission_type = 2;
    let mut create = CreateAclsRequest::default();
    create.creations.push(acl);
    let made: CreateAclsResponse = call(&mut c, ApiKey::CreateAcls, 3, &create);
    assert_eq!(made.results[0].error_code, 0);
    let closed = home(&other);
    assert!(closed.starts_with("HTTP/1.1 403"), "{closed}");
    assert!(
        closed.contains("\"code\":\"CONSOLE_CLOSED_TO_NETWORK\""),
        "{closed}"
    );
    assert!(
        closed.contains(&format!("at http://127.0.0.1:{port}/")),
        "{closed}"
    );
    // The page itself still loads there, so it can say why.
    let index = http(&other, &format!("GET /live HTTP/1.1\r\nHost: {other}"), "");
    assert!(index.starts_with("HTTP/1.1 200"), "{index:.300}");
    assert!(home(&local).starts_with("HTTP/1.1 200"));
    logged(
        &broker,
        "the broker now has ACLs, so the console no longer answers other machines",
    );
    // Every ACL deleted: open again, and said once.
    let mut every = DeleteAclsFilter::default();
    every.resource_type_filter = 1;
    every.pattern_type_filter = 1;
    every.operation = 1;
    every.permission_type = 1;
    every.resource_name_filter = None;
    every.principal_filter = None;
    every.host_filter = None;
    let mut delete = DeleteAclsRequest::default();
    delete.filters.push(every);
    let gone: DeleteAclsResponse = call(&mut c, ApiKey::DeleteAcls, 3, &delete);
    assert_eq!(gone.filter_results[0].matching_acls.len(), 1);
    let again = home(&other);
    assert!(again.starts_with("HTTP/1.1 200"), "{again}");
    let log = logged(&broker, "the console answers other machines again");
    assert_eq!(
        log.matches("no longer answers other machines").count(),
        1,
        "{log}"
    );
    assert!(broker.terminate().success());
    let _ = std::fs::remove_dir_all(&data);
}
