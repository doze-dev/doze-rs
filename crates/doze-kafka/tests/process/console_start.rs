//! The console's start-up in the binary, the audit's fixes (A6, A10, A11, A12, A21 to A29): the banner through the binary, the
//! address bound before anything is created, a flag that is not a boolean, a console that says it is off, the descriptor budget,
//! and a process that cannot hang a test.

#[cfg(feature = "console")]
use super::console::SASL;
#[cfg(feature = "console")]
use super::console::{console_addr, http};
use super::*;

/// What the shell of a console that listens on `addr` is told (`/api/v1/shell`: the top bar and the banner), asked for by
/// the loopback address (a console bound to every interface answers there too).
#[cfg(feature = "console")]
fn home_of(addr: &str) -> String {
    let port = addr.rsplit(':').next().unwrap();
    let host = format!("127.0.0.1:{port}");
    http(
        &host,
        &format!("GET /api/v1/shell HTTP/1.1\r\nHost: {host}\r\nX-Console: 1"),
        "",
    )
}

#[test]
#[cfg(feature = "console")]
fn a_console_served_beyond_loopback_says_so_on_every_page_it_serves() {
    let data = scratch("console-banner");
    // Nothing protects the broker and Kafka is on the network too, so the console may listen on every interface; the page still
    // says who can reach it.
    let mut broker = start_with(
        &data,
        &["--listen", "0.0.0.0:0", "--console-addr", "0.0.0.0:0"],
    );
    let addr = console_addr(&broker);
    assert!(addr.starts_with("0.0.0.0:"), "{addr}");
    let home = home_of(&addr);
    assert!(home.starts_with("HTTP/1.1 200"), "{home}");
    // The top bar draws its warning (served beyond this machine with no login) from this.
    assert!(home.contains("\"unprotected\":true"), "{home}");
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
    assert!(home.contains("\"unprotected\":true"), "{home}");
    assert!(!home.contains("planted"), "{home}");
    assert!(broker.terminate().success());
    let _ = std::fs::remove_dir_all(&data);
    // On loopback there is no banner.
    let data = scratch("console-no-banner");
    let mut broker = start(&data);
    let addr = console_addr(&broker);
    let home = home_of(&addr);
    assert!(home.starts_with("HTTP/1.1 200"), "{home}");
    assert!(home.contains("\"unprotected\":false"), "{home}");
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
    assert!(
        said.contains("the console address cannot be used"),
        "{said}"
    );
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
    assert!(
        said.contains("--console takes true, false, 1, 0, yes, no, on or off"),
        "{said}"
    );
    assert!(!data.exists());
}

#[test]
#[cfg(feature = "console")]
fn a_console_that_is_off_says_so_in_the_start_banner() {
    for flag in [
        "--console=false",
        "--console=off",
        "--console=0",
        "--console=NO",
    ] {
        let data = scratch("console-off-says");
        let mut broker = start_with(&data, &[flag]);
        std::thread::sleep(Duration::from_millis(300));
        let log = broker.log.lock().unwrap().clone();
        assert!(
            log.contains("the console is off (--console=false)"),
            "{flag}: {log}"
        );
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
    assert!(
        outcome.is_err(),
        "a process that was still running was waited for"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

/// `(partitions kept open, limit on open files)` from the line the binary logs when the limit lowers the open partitions.
fn kept_open(broker: &Broker) -> (u64, u64) {
    let log = broker.log.lock().unwrap().clone();
    let line = log
        .lines()
        .find(|l| l.contains("keeping at most"))
        .unwrap_or_else(|| panic!("the binary did not say it lowered the open partitions: {log}"));
    let number = |after: &str| -> u64 {
        let rest = &line[line.find(after).unwrap() + after.len()..];
        rest.split(|c: char| !c.is_ascii_digit())
            .next()
            .unwrap()
            .parse()
            .unwrap()
    };
    (
        number("keeping at most "),
        number("the limit on open files is "),
    )
}

#[test]
fn the_consoles_connections_come_out_of_the_descriptor_budget_of_the_partitions() {
    // Unbounded partitions are bounded by what the limit leaves: the limit, less 256 kept back, less the Kafka connections
    // that may be open (2,048) and, with a console, its listener and its 256 connections, shared four to a partition.
    let (kept, limit) = {
        let data = scratch("console-budget-on");
        let mut broker = start_with(&data, &["--max-open-partitions", "0"]);
        let seen = kept_open(&broker);
        // (The signal handler is installed once everything is serving; a TERM before that is the default action.)
        std::thread::sleep(Duration::from_millis(400));
        assert!(broker.terminate().success());
        let _ = std::fs::remove_dir_all(&data);
        seen
    };
    let console = if cfg!(feature = "console") { 257 } else { 0 };
    assert_eq!(kept, (limit - 256 - 2048 - console) / 4, "limit {limit}");
    let data = scratch("console-budget-off");
    let mut broker = start_with(&data, &["--max-open-partitions", "0", "--console=false"]);
    let (kept, limit) = kept_open(&broker);
    assert_eq!(kept, (limit - 256 - 2048) / 4, "limit {limit}, no console");
    std::thread::sleep(Duration::from_millis(400));
    assert!(broker.terminate().success());
    let _ = std::fs::remove_dir_all(&data);
}
