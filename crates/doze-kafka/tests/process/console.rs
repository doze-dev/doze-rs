//! The start-up rule on the console flags, in the binary: refused with exit 2, or accepted and said once.

use super::*;

const SASL: [&str; 4] = ["--sasl-mechanisms", "PLAIN", "--sasl-user", "u:pw-planted"];

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
        said.contains("the console has no login and is plain HTTP, and SASL is on: serving it on 0.0.0.0:9093 would"),
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
        log.contains("the console is served on 0.0.0.0:0 with no login and over plain HTTP although SASL is on (--console-unprotected)"),
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

/// The address the console says it is listening on.
#[cfg(feature = "console")]
fn console_addr(broker: &Broker) -> String {
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
fn http(addr: &str, head: &str, body: &str) -> String {
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
    let home = http(&addr, &format!("GET / HTTP/1.1\r\nHost: {addr}"), "");
    assert!(home.starts_with("HTTP/1.1 200"), "{home}");
    assert!(
        home.contains(&format!("127.0.0.1:{}", broker.port)),
        "the page names the Kafka address"
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
    let post = format!("POST /connect/advertised/check HTTP/1.1\r\nHost: {addr}");
    let bare = http(&addr, &post, "back=home");
    assert!(
        bare.starts_with("HTTP/1.1 403"),
        "a post with no proof of origin: {bare}"
    );
    let cross = http(
        &addr,
        &format!("{post}\r\nOrigin: http://evil.example"),
        "back=home",
    );
    assert!(cross.starts_with("HTTP/1.1 403"), "{cross}");
    let own = http(
        &addr,
        &format!("{post}\r\nOrigin: http://{addr}"),
        "back=home",
    );
    assert!(own.starts_with("HTTP/1.1 303"), "{own}");
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
        "GET /events?watch=home-live HTTP/1.1\r\nHost: {addr}\r\n\r\n"
    )
    .unwrap();
    let mut seen = Vec::new();
    let mut buf = [0_u8; 512];
    while !String::from_utf8_lossy(&seen).contains("event: tick") {
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
    assert!(
        under.contains("href=\"/kafka/connect/advertised\""),
        "{under}"
    );
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

/// The page at `/` of a console that listens on `addr`, asked for by the loopback address (a console bound to every
/// interface answers there too).
#[cfg(feature = "console")]
fn home_of(addr: &str) -> String {
    let port = addr.rsplit(':').next().unwrap();
    let host = format!("127.0.0.1:{port}");
    http(&host, &format!("GET / HTTP/1.1\r\nHost: {host}"), "")
}

#[test]
#[cfg(feature = "console")]
fn a_console_served_beyond_loopback_says_so_on_every_page_it_serves() {
    let data = scratch("console-banner");
    // Nothing protects the broker, so the console may listen on every interface; the page still says who can reach it.
    let mut broker = start_with(&data, &["--console-addr", "0.0.0.0:0"]);
    let addr = console_addr(&broker);
    assert!(addr.starts_with("0.0.0.0:"), "{addr}");
    let home = home_of(&addr);
    assert!(home.starts_with("HTTP/1.1 200"), "{home}");
    assert!(home.contains("Served beyond loopback, no login."), "{home}");
    assert!(
        home.contains("Anyone who can reach this address can use the console"),
        "{home}"
    );
    assert!(broker.terminate().success());
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
#[cfg(feature = "console")]
fn the_banner_shows_with_sasl_and_unprotected_and_is_absent_on_loopback() {
    let data = scratch("console-banner-sasl");
    let flags: Vec<&str> = SASL
        .iter()
        .copied()
        .chain(["--console-addr", "0.0.0.0:0", "--console-unprotected"])
        .collect();
    let mut broker = start_with(&data, &flags);
    let addr = console_addr(&broker);
    let home = home_of(&addr);
    assert!(home.contains("Served beyond loopback, no login."), "{home}");
    assert!(!home.contains("planted"), "{home}");
    assert!(broker.terminate().success());
    let _ = std::fs::remove_dir_all(&data);
    // On loopback there is no banner.
    let data = scratch("console-no-banner");
    let mut broker = start(&data);
    let addr = console_addr(&broker);
    let home = http(&addr, &format!("GET / HTTP/1.1\r\nHost: {addr}"), "");
    assert!(home.starts_with("HTTP/1.1 200"), "{home}");
    assert!(!home.contains("Served beyond loopback"), "{home}");
    assert!(broker.terminate().success());
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
#[cfg(feature = "console")]
fn a_console_address_that_cannot_be_bound_stops_the_start_before_anything_is_created() {
    let data = scratch("console-taken-first");
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = taken.local_addr().unwrap().to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_doze-kafka"))
        .args(["--listen", "127.0.0.1:0", "--data-dir"])
        .arg(&data)
        .args(["--console-addr", &addr])
        .output_within(PROCESS_WAIT);
    let said = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{said}");
    assert!(said.contains("the console address cannot be used"), "{said}");
    // SEC-247: before anything is opened. Not the directory, not the lock file in it.
    assert!(
        !data.exists(),
        "the data directory was created before the address was refused: {:?}",
        std::fs::read_dir(&data).map(|d| d.flatten().map(|e| e.file_name()).collect::<Vec<_>>())
    );
}

#[test]
fn a_console_flag_that_is_not_a_boolean_exits_2_before_anything_is_created() {
    let data = scratch("console-not-bool");
    let output = Command::new(env!("CARGO_BIN_EXE_doze-kafka"))
        .args(["--listen", "127.0.0.1:0", "--data-dir"])
        .arg(&data)
        .arg("--console=maybe")
        .output_within(PROCESS_WAIT);
    let said = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{said}");
    assert!(said.contains("--console takes true, false, 1, 0, yes, no, on or off"), "{said}");
    assert!(!data.exists());
}

#[test]
#[cfg(feature = "console")]
fn a_console_that_is_off_says_so_in_the_start_banner() {
    for flag in ["--console=false", "--console=off", "--console=0", "--console=NO"] {
        let data = scratch("console-off-says");
        let mut broker = start_with(&data, &[flag]);
        std::thread::sleep(Duration::from_millis(300));
        let log = broker.log.lock().unwrap().clone();
        assert!(log.contains("the console is off (--console=false)"), "{flag}: {log}");
        assert!(!log.contains("the console is listening"), "{flag}: {log}");
        assert!(broker.terminate().success());
        let _ = std::fs::remove_dir_all(&data);
    }
}

#[test]
fn a_process_that_never_exits_fails_the_test_in_seconds_and_not_in_minutes() {
    let started = Instant::now();
    let outcome = std::panic::catch_unwind(|| {
        Command::new("sleep")
            .arg("60")
            .output_within(Duration::from_secs(1))
    });
    assert!(outcome.is_err(), "a process that was still running was waited for");
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
}
