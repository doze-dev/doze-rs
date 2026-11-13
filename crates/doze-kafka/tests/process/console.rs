//! The start-up rule on the console flags, in the binary: refused with exit 2, or accepted and said once.

use super::*;

pub(super) const SASL: [&str; 4] = ["--sasl-mechanisms", "PLAIN", "--sasl-user", "u:pw-planted"];

#[test]
fn sasl_and_a_console_beyond_loopback_stop_the_start_with_exit_2_unless_unprotected() {
    let data = scratch("console-refused");
    let output = Command::new(env!("CARGO_BIN_EXE_doze-kafka"))
        .args(["--listen", "127.0.0.1:0", "--data-dir"])
        .arg(&data)
        .args(SASL)
        .args(["--console-addr", "0.0.0.0:9093"])
        .output_within(PROCESS_WAIT);
    let said = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{said}");
    assert!(
        said.contains("the console has no login and is plain HTTP, and SASL is on and --listen 127.0.0.1:0 keeps Kafka on this machine: serving it on 0.0.0.0:9093 would"),
        "{said}"
    );
    assert!(said.contains("--console-unprotected"), "{said}");
    assert!(!said.contains("planted"), "{said}");
    // Nothing was opened: the refusal comes before the data directory is touched.
    assert!(!data.exists());
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn a_console_address_that_is_no_host_and_port_exits_2_too() {
    let data = scratch("console-bad-addr");
    let output = Command::new(env!("CARGO_BIN_EXE_doze-kafka"))
        .args(["--listen", "127.0.0.1:0", "--data-dir"])
        .arg(&data)
        .args(["--console-addr", "nonsense"])
        .output_within(PROCESS_WAIT);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("is not host:port"));
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn unprotected_starts_the_broker_and_the_log_says_what_is_open() {
    let data = scratch("console-unprotected");
    let flags: Vec<&str> = SASL
        .iter()
        .copied()
        .chain([
            "--console-addr",
            "0.0.0.0:0",
            "--console-unprotected",
            "--console-allow-host",
            "dev.example.com",
        ])
        .collect();
    let mut broker = start_with(&data, &flags);
    let log = broker.log.lock().unwrap().clone();
    assert!(
        log.contains("the console is served on 0.0.0.0:0 with no login and over plain HTTP although SASL is on and --listen 127.0.0.1:0 keeps Kafka on this machine (--console-unprotected)"),
        "{log}"
    );
    // A build with a console acts on the flags; one without says it ignores them.
    assert_eq!(
        log.contains("is accepted and ignored"),
        !cfg!(feature = "console"),
        "{log}"
    );
    assert!(!log.contains("planted"), "{log}");
    assert!(broker.terminate().success());
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn acls_in_force_or_kafka_on_loopback_and_a_console_on_the_network_stop_the_start_with_exit_2() {
    // The audit's case: Kafka on loopback with ACLs denying, the console on every interface. No SASL, no TLS.
    for (flags, what) in [
        (
            &["--acl-default", "deny"][..],
            "and ACLs are in force (--acl-default deny) and --listen 127.0.0.1:0 keeps Kafka on this machine: serving it on 0.0.0.0:0 would let anyone who can reach that address do what the ACLs refuse (the console acts as a super user) and reach from the network a broker that Kafka clients can reach only from this machine.",
        ),
        (
            &[],
            "and --listen 127.0.0.1:0 keeps Kafka on this machine: serving it on 0.0.0.0:0 would",
        ),
    ] {
        let data = scratch("console-wider-door");
        let output = Command::new(env!("CARGO_BIN_EXE_doze-kafka"))
            .args(["--listen", "127.0.0.1:0", "--data-dir"])
            .arg(&data)
            .args(["--console-addr", "0.0.0.0:0"])
            .args(flags)
            .output_within(PROCESS_WAIT);
        let said = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{flags:?}: {said}");
        assert!(said.contains(what), "{flags:?}: {said}");
        for way_out in [
            "Keep --console-addr on loopback",
            "--console=false",
            "--console-unprotected",
        ] {
            assert!(said.contains(way_out), "{flags:?}: {said}");
        }
        assert!(!data.exists());
    }
}

#[test]
#[cfg(feature = "console")]
fn an_open_broker_on_the_network_serves_its_console_there_with_the_lab_off_unless_asked() {
    let data = scratch("console-open-network");
    let mut broker = start_with(
        &data,
        &["--listen", "0.0.0.0:0", "--console-addr", "0.0.0.0:0"],
    );
    let addr = console_addr(&broker);
    let port = addr.rsplit(':').next().unwrap();
    let host = format!("127.0.0.1:{port}");
    let lab = http(
        &host,
        &format!("GET /api/v1/lab HTTP/1.1\r\nHost: {host}\r\nX-Console: 1"),
        "",
    );
    let log = broker.log.lock().unwrap().clone();
    assert!(
        log.contains("the console is served on 0.0.0.0:0 with no login and over plain HTTP: anyone who can reach that address can do what any Kafka client of this broker can"),
        "{log}"
    );
    assert!(
        log.contains("the Lab is off because the console is served beyond loopback (--lab=true switches it on)"),
        "{log}"
    );
    assert!(!log.contains("the Lab is on"), "{log}");
    assert!(lab.contains("\"off\":true"), "{lab}");
    assert!(broker.terminate().success());
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
#[cfg(feature = "console")]
fn acls_stored_in_the_data_directory_stop_a_console_on_the_network_unless_unprotected() {
    use kafka_protocol::messages::CreateAclsRequest;
    use kafka_protocol::messages::CreateAclsResponse;
    use kafka_protocol::messages::create_acls_request::AclCreation;
    let data = scratch("console-stored-acls");
    // An open broker stores an ACL (anybody may, with the allow default and no login).
    let broker = start(&data);
    let mut c = broker.connect();
    let mut acl = AclCreation::default();
    acl.resource_type = 2;
    acl.resource_name = StrBytes::from_static_str("t");
    acl.resource_pattern_type = 3;
    acl.principal = StrBytes::from_static_str("User:someone");
    acl.host = StrBytes::from_static_str("*");
    acl.operation = 8;
    acl.permission_type = 2;
    let mut create = CreateAclsRequest::default();
    create.creations.push(acl);
    let made: CreateAclsResponse = call(&mut c, ApiKey::CreateAcls, 3, &create);
    assert_eq!(made.results[0].error_code, 0);
    drop((c, broker));
    // Kafka on the network and nothing in the flags guards it, but the stored ACL does: the console may not get round it.
    let output = Command::new(env!("CARGO_BIN_EXE_doze-kafka"))
        .args(["--listen", "0.0.0.0:0", "--data-dir"])
        .arg(&data)
        .args(["--console-addr", "0.0.0.0:0"])
        .output_within(PROCESS_WAIT);
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{said}");
    assert!(
        said.contains("and ACLs are in force (1 stored in the data directory): serving it on 0.0.0.0:0 would let anyone who can reach that address do what the ACLs refuse"),
        "{said}"
    );
    // Accepted, the start says what is open.
    let mut broker = start_with(
        &data,
        &[
            "--listen",
            "0.0.0.0:0",
            "--console-addr",
            "0.0.0.0:0",
            "--console-unprotected",
        ],
    );
    let log = broker.log.lock().unwrap().clone();
    assert!(
        log.contains(
            "although ACLs are in force (1 stored in the data directory) (--console-unprotected)"
        ),
        "{log}"
    );
    assert!(broker.terminate().success());
    let _ = std::fs::remove_dir_all(&data);
}

/// The address the console says it is listening on.
#[cfg(feature = "console")]
pub(super) fn console_addr(broker: &Broker) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let log = broker.log.lock().unwrap().clone();
        if let Some(line) = log.lines().find(|l| l.contains("the console is listening")) {
            let at = line.find("addr=").unwrap() + 5;
            return line[at..].split_whitespace().next().unwrap().to_owned();
        }
        assert!(
            Instant::now() < deadline,
            "the console never said it was listening: {log}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// One HTTP/1.1 request, answered to the end (the connection is closed after it).
#[cfg(feature = "console")]
pub(super) fn http(addr: &str, head: &str, body: &str) -> String {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let request = format!(
        "{head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(request.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

#[test]
#[cfg(feature = "console")]
fn the_console_serves_its_pages_beside_the_broker_and_refuses_what_it_should() {
    let data = scratch("console-serves");
    let mut broker = start(&data);
    let addr = console_addr(&broker);
    let spa = http(
        &addr,
        &format!("GET /topics/orders HTTP/1.1\r\nHost: {addr}"),
        "",
    );
    assert!(spa.starts_with("HTTP/1.1 200"), "{spa}");
    assert!(
        spa.contains("<meta name=\"doze-base\""),
        "the React console answers every page address: {spa}"
    );
    // The API the pages read: Home names the Kafka address.
    let home = http(
        &addr,
        &format!("GET /api/v1/home HTTP/1.1\r\nHost: {addr}\r\nX-Console: 1"),
        "",
    );
    assert!(home.starts_with("HTTP/1.1 200"), "{home}");
    assert!(
        home.contains(&format!("127.0.0.1:{}", broker.port)),
        "Home names the Kafka address"
    );
    assert!(
        home.to_ascii_lowercase()
            .contains("content-security-policy: default-src 'self'"),
        "{home}"
    );
    assert!(
        http(&addr, &format!("GET /connect HTTP/1.1\r\nHost: {addr}"), "")
            .starts_with("HTTP/1.1 200")
    );
    let foreign = http(&addr, "GET / HTTP/1.1\r\nHost: evil.example", "");
    assert!(foreign.starts_with("HTTP/1.1 403"), "{foreign}");
    assert!(foreign.contains("--console-allow-host"), "{foreign}");
    let post = format!("POST /api/v1/probe HTTP/1.1\r\nHost: {addr}\r\nX-Console: 1");
    let bare = http(&addr, &post, "");
    assert!(
        bare.starts_with("HTTP/1.1 403"),
        "a post with no proof of origin: {bare}"
    );
    let cross = http(&addr, &format!("{post}\r\nOrigin: http://evil.example"), "");
    assert!(cross.starts_with("HTTP/1.1 403"), "{cross}");
    let own = http(&addr, &format!("{post}\r\nOrigin: http://{addr}"), "");
    assert!(own.starts_with("HTTP/1.1 200"), "{own}");
    // An address with no page of its own is the app's, which says Not here.
    let unknown = http(
        &addr,
        &format!("GET /no-such-page HTTP/1.1\r\nHost: {addr}"),
        "",
    );
    assert!(unknown.contains("<meta name=\"doze-base\""), "{unknown}");
    assert!(broker.terminate().success());
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
#[cfg(feature = "console")]
fn a_stopping_broker_does_not_wait_for_an_open_console_stream() {
    let data = scratch("console-stream");
    let mut broker = start(&data);
    let addr = console_addr(&broker);
    let mut stream = TcpStream::connect(&addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(
        stream,
        "GET /api/v1/tail/live HTTP/1.1\r\nHost: {addr}\r\nX-Console: 1\r\n\r\n"
    )
    .unwrap();
    let mut seen = Vec::new();
    let mut buf = [0_u8; 512];
    while !String::from_utf8_lossy(&seen).contains("event: ready") {
        let n = stream.read(&mut buf).unwrap();
        assert!(
            n > 0,
            "the stream closed before its first event: {}",
            String::from_utf8_lossy(&seen)
        );
        seen.extend_from_slice(&buf[..n]);
    }
    // The stream is open; SIGTERM must still stop the process (terminate waits ten seconds and fails after).
    let started = Instant::now();
    assert!(broker.terminate().success());
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "{:?}",
        started.elapsed()
    );
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn console_false_leaves_the_broker_as_it_was() {
    let data = scratch("console-off");
    let mut broker = start_with(&data, &["--console=false"]);
    std::thread::sleep(Duration::from_millis(300));
    let log = broker.log.lock().unwrap().clone();
    // It says that it is off (one line), and does nothing else: no listener, no address.
    assert_eq!(
        log.contains("the console is off (--console=false)"),
        cfg!(feature = "console"),
        "{log}"
    );
    assert!(!log.contains("the console is listening"), "{log}");
    assert!(broker.terminate().success());
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
#[cfg(feature = "console")]
fn a_console_address_that_is_taken_stops_the_start_with_exit_2() {
    let data = scratch("console-taken");
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = taken.local_addr().unwrap().to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_doze-kafka"))
        .args(["--listen", "127.0.0.1:0", "--data-dir"])
        .arg(&data)
        .args(["--console-addr", &addr])
        .output_within(PROCESS_WAIT);
    let said = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{said}");
    assert!(
        said.contains("the console address cannot be used"),
        "{said}"
    );
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
#[cfg(feature = "console")]
fn a_prefix_moves_every_page_under_it() {
    let data = scratch("console-prefix");
    let mut broker = start_with(&data, &["--console-prefix", "/kafka"]);
    let addr = console_addr(&broker);
    let under = http(
        &addr,
        &format!("GET /kafka/connect HTTP/1.1\r\nHost: {addr}"),
        "",
    );
    assert!(under.starts_with("HTTP/1.1 200"), "{under}");
    assert!(under.contains("content=\"/kafka\""), "{under}");
    assert!(under.contains("src=\"/kafka/assets/"), "{under}");
    let api = http(
        &addr,
        &format!("GET /kafka/api/v1/shell HTTP/1.1\r\nHost: {addr}\r\nX-Console: 1"),
        "",
    );
    assert!(api.starts_with("HTTP/1.1 200"), "{api}");
    let bare = http(&addr, &format!("GET /connect HTTP/1.1\r\nHost: {addr}"), "");
    assert!(bare.starts_with("HTTP/1.1 404"), "{bare}");
    assert!(broker.terminate().success());
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
#[cfg(not(feature = "console"))]
fn a_build_without_a_console_says_it_ignores_the_console_flags_and_serves_none() {
    let data = scratch("console-lean");
    let mut broker = start_with(
        &data,
        &["--console-addr", "127.0.0.1:0", "--console-prefix", "/k"],
    );
    std::thread::sleep(Duration::from_millis(300));
    let log = broker.log.lock().unwrap().clone();
    assert!(
        log.contains("--console-addr is accepted and ignored: this build has no console"),
        "{log}"
    );
    assert!(!log.contains("the console is listening"), "{log}");
    assert!(broker.terminate().success());
    let _ = std::fs::remove_dir_all(&data);
}
