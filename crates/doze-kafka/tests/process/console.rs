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
        .output()
        .unwrap();
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
        .output()
        .unwrap();
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
            "0.0.0.0:9093",
            "--console-unprotected",
            "--console-allow-host",
            "dev.example.com",
        ])
        .collect();
    let mut broker = start_with(&data, &flags);
    let log = broker.log.lock().unwrap().clone();
    assert!(
        log.contains("the console is served on 0.0.0.0:9093 with no login and over plain HTTP although SASL is on (--console-unprotected)"),
        "{log}"
    );
    assert!(
        log.contains("--console-allow-host is accepted and ignored"),
        "{log}"
    );
    assert!(!log.contains("planted"), "{log}");
    assert!(broker.terminate().success());
    let _ = std::fs::remove_dir_all(&data);
}
