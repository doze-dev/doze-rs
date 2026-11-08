use super::*;

fn args(extra: &[&str]) -> Result<Args, clap::Error> {
    Args::try_parse_from(["doze-kafka"].into_iter().chain(extra.iter().copied()))
}

#[test]
fn the_flags_the_conformance_harness_passes_are_all_accepted() {
    let passed = [
        "--listen",
        "127.0.0.1:45123",
        "--data-dir",
        "/tmp/d/data",
        "--meta-db",
        "/tmp/d/meta.db",
        "--group-initial-rebalance-delay",
        "3s",
        "--consumer-session-timeout",
        "45s",
        "--offsets-retention",
        "168h0m0s",
        "--acl-default",
        "deny",
        "--acl-super-user",
        "User:admin,User:root",
        "--acl-super-user",
        "User:x",
        "--console",
        "--console-addr",
        "127.0.0.1:0",
    ];
    let parsed = args(&passed).unwrap();
    let settings = parsed.settings().unwrap();
    assert_eq!(settings.server.listen.port(), 45123);
    assert_eq!(parsed.ignored(), ["--console"]);
}

#[test]
fn the_defaults_are_a_local_broker_with_no_retention_limit() {
    let settings = args(&[]).unwrap().settings().unwrap();
    assert_eq!(settings.server.listen, "127.0.0.1:9092".parse().unwrap());
    assert_eq!(settings.broker.defaults.retention_ms, -1);
    assert_eq!(settings.broker.defaults.retention_bytes, -1);
    assert_eq!(settings.broker.fsync_interval, Duration::from_millis(500));
    assert!(settings.broker.auto_create_topics);
}

#[test]
fn the_values_that_make_a_broker_that_cannot_work_are_refused_with_words() {
    for (flags, why) in [
        (&["--kafka-version", "5"][..], "--kafka-version"),
        (&["--segment-bytes", "0"], "--segment-bytes"),
        (
            &["--default-partitions", "5", "--max-partitions", "2"],
            "--default-partitions",
        ),
        (&["--worker-threads", "0"], "--worker-threads"),
    ] {
        let message = args(flags).unwrap().settings().unwrap_err().to_string();
        assert!(message.contains(why), "{flags:?}: {message}");
    }
}

#[test]
fn authentication_is_refused_and_not_ignored_because_ignoring_it_would_leave_the_broker_open() {
    for flags in [
        &["--sasl-mechanisms", "PLAIN"][..],
        &["--sasl-user", "u:p"],
        &["--tls-cert", "c.pem"],
    ] {
        assert!(args(flags).unwrap().settings().is_err(), "{flags:?}");
    }
}

#[test]
fn a_zero_retention_is_no_limit_and_a_value_is_kept() {
    let s = args(&["--retention-ms", "60000", "--retention-bytes", "0"])
        .unwrap()
        .settings()
        .unwrap();
    assert_eq!(
        (
            s.broker.defaults.retention_ms,
            s.broker.defaults.retention_bytes
        ),
        (60000, -1)
    );
}
