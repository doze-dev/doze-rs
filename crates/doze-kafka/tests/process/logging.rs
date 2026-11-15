//! `RUST_LOG` and `--log-level` in the binary: a level or a list of `target=level` pairs is honoured; a span or field filter
//! stops the start with exit 2 and one line saying what is supported.

use super::*;

#[test]
fn a_span_or_field_filter_in_rust_log_stops_the_start_and_says_what_is_supported() {
    for spec in [
        "kafka_broker[fetch]=debug",
        "[produce{topic=orders}]=trace",
        "info,kafka_server[{peer}]=debug",
    ] {
        let data = scratch("log-span");
        let output = Command::new(env!("CARGO_BIN_EXE_doze-kafka"))
            .args(["--listen", "127.0.0.1:0", "--data-dir"])
            .arg(&data)
            .env("RUST_LOG", spec)
            .output_within(PROCESS_WAIT);
        let said = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{spec}: {said}");
        assert_eq!(said.trim_end().lines().count(), 1, "{spec}: {said}");
        assert!(said.contains("RUST_LOG"), "{said}");
        assert!(said.contains("span and field filters"), "{said}");
        assert!(said.contains("target=level"), "{said}");
        // Refused before anything was opened.
        assert!(!data.exists(), "{spec}");
    }
}

#[test]
fn a_target_list_in_rust_log_and_a_level_in_log_level_are_both_honoured() {
    // "console flags" is a debug line of the binary's own target, written before it says it is listening.
    for (flags, env) in [
        (&[][..], ("RUST_LOG", "warn,doze_kafka=debug")),
        (&["--log-level", "debug"][..], ("RUST_LOG", "")),
    ] {
        let data = scratch("log-list");
        let mut broker = start_with_env(&data, flags, &[env]);
        let said = broker.log.lock().unwrap().clone();
        assert!(said.contains("console flags"), "{flags:?} {env:?}: {said}");
        assert!(broker.terminate().success());
        let _ = std::fs::remove_dir_all(&data);
    }
}
