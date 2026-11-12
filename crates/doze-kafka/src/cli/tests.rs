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
    // With a console in the build the console flags are acted on; without one they are named as ignored.
    let ignored: &[&str] = if cfg!(feature = "console") {
        &[]
    } else {
        &["--console", "--console-addr"]
    };
    assert_eq!(parsed.ignored(), ignored);
    assert!(!settings.broker.acl_default_allow);
    assert_eq!(
        settings
            .broker
            .super_users
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["User:admin", "User:root", "User:x"]
    );
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

#[cfg(not(feature = "tls"))]
#[test]
fn tls_is_refused_and_not_ignored_because_ignoring_it_would_leave_the_channel_open() {
    for flags in [
        &["--tls-cert", "c.pem"][..],
        &["--tls-key", "k.pem"],
        &["--tls-cert", "c.pem", "--tls-key", "k.pem"],
    ] {
        let said = args(flags).unwrap().settings().unwrap_err().to_string();
        assert!(said.contains("--features tls"), "{flags:?}: {said}");
        assert!(said.contains("tls-cert"), "{flags:?}: {said}");
    }
}

/// The settings of `flags`, as the user would read the refusal: or the authenticator.
fn sasl(flags: &[&str]) -> Result<Option<std::sync::Arc<kafka_auth::Authenticator>>, String> {
    let args = args(flags).unwrap();
    args.settings()
        .map(|s| s.server.sasl)
        .map_err(|e| format!("{e:#}"))
}

#[test]
fn no_sasl_flags_means_no_login() {
    assert!(sasl(&[]).unwrap().is_none());
}

#[test]
fn plain_with_users_turns_login_on_and_the_password_may_hold_colons() {
    let auth = sasl(&[
        "--sasl-mechanisms",
        "PLAIN",
        "--sasl-user",
        "admin:admin-secret",
        "--sasl-user",
        "alice:a:b:c",
    ])
    .unwrap()
    .unwrap();
    assert_eq!(auth.offered(), [kafka_auth::Mechanism::Plain]);
    assert_eq!(auth.user_count(), 2);
    // The split is at the first colon, so the rest is the password.
    assert!(auth.plain(b"\0alice\0a:b:c").is_ok());
    assert!(auth.plain(b"\0alice\0b:c").is_err());
    assert!(auth.plain(b"\0admin\0admin-secret").is_ok());
}

#[test]
fn sasl_flags_that_cannot_work_are_refused_with_the_go_editions_words() {
    let cases: [(&[&str], &str); 8] = [
        (
            &["--sasl-user", "u:p"],
            "no mechanism is enabled; add --sasl-mechanisms",
        ),
        (
            &["--sasl-mechanisms", "PLAIN"],
            "no credentials are configured",
        ),
        (
            &["--sasl-mechanisms", "plain", "--sasl-user", "u:p"],
            "unknown SASL mechanism \"plain\"; supported: PLAIN, SCRAM-SHA-256, SCRAM-SHA-512",
        ),
        (
            &["--sasl-mechanisms", "PLAIN,", "--sasl-user", "u:p"],
            "unknown SASL mechanism \"\"",
        ),
        (
            &[
                "--sasl-mechanisms",
                "PLAIN",
                "--sasl-user",
                "nocolon-planted",
            ],
            "--sasl-user: not a valid value (expected user:password)",
        ),
        (
            &[
                "--sasl-mechanisms",
                "PLAIN",
                "--sasl-user",
                ":secret-planted",
            ],
            "SASL credential has an empty username",
        ),
        (
            &["--sasl-mechanisms", "PLAIN", "--sasl-user", "alice:"],
            "SASL user \"alice\" has an empty password",
        ),
        (
            &[
                "--sasl-mechanisms",
                "PLAIN",
                "--sasl-user",
                "a:one-planted",
                "--sasl-user",
                "a:two-planted",
            ],
            "SASL user \"a\" is given more than once",
        ),
    ];
    for (flags, why) in cases {
        let said = sasl(flags).unwrap_err();
        assert!(said.contains(why), "{flags:?}: {said}");
        assert!(!said.contains("planted"), "{said}");
    }
}

#[test]
fn scram_mechanisms_are_offered_in_javas_order_whatever_order_the_flag_has() {
    use kafka_auth::Mechanism::{Plain, ScramSha256, ScramSha512};
    let cases: [(&str, &[kafka_auth::Mechanism]); 4] = [
        ("SCRAM-SHA-256", &[ScramSha256]),
        ("SCRAM-SHA-256,SCRAM-SHA-512", &[ScramSha512, ScramSha256]),
        (
            "PLAIN,SCRAM-SHA-512,SCRAM-SHA-256",
            &[Plain, ScramSha512, ScramSha256],
        ),
        (
            "SCRAM-SHA-256,PLAIN,SCRAM-SHA-512",
            &[Plain, ScramSha512, ScramSha256],
        ),
    ];
    for (flag, offered) in cases {
        let auth = sasl(&["--sasl-mechanisms", flag, "--sasl-user", "alice:pw"])
            .unwrap()
            .unwrap();
        assert_eq!(auth.offered(), offered, "{flag}");
    }
    // The wrong case of a SCRAM name is as unknown as the wrong case of PLAIN.
    let said = sasl(&["--sasl-mechanisms", "scram-sha-256", "--sasl-user", "u:p"]).unwrap_err();
    assert!(said.contains("unknown SASL mechanism"), "{said}");
}

#[test]
fn a_scram_user_logs_in_with_the_password_of_the_flag_and_with_no_other() {
    use kafka_auth::scram::Kind;
    use kafka_auth::scram::client::Client;
    let flags = [
        "--sasl-mechanisms",
        "PLAIN,SCRAM-SHA-512,SCRAM-SHA-256",
        "--sasl-user",
        "alice:a:b:c",
    ];
    let auth = sasl(&flags).unwrap().unwrap();
    for (mechanism, kind) in [
        (kafka_auth::Mechanism::ScramSha256, Kind::Sha256),
        (kafka_auth::Mechanism::ScramSha512, Kind::Sha512),
    ] {
        for (password, right) in [("a:b:c", true), ("b:c", false)] {
            let me = Client::new(kind, "alice", password, "nonce.");
            let (challenge, awaiting) = auth.scram_first(mechanism, &me.first()).unwrap();
            let last = me.last(&challenge, None, "");
            assert_eq!(awaiting.finish(&last.message).is_ok(), right, "{password}");
        }
    }
}

#[test]
fn the_password_of_a_user_is_in_no_debug_output_of_the_arguments_or_the_settings() {
    let parsed = args(&[
        "--sasl-mechanisms",
        "PLAIN",
        "--sasl-user",
        "alice:hunter2-planted",
    ])
    .unwrap();
    let shown = format!("{parsed:?} {:?}", parsed.settings().unwrap());
    assert!(shown.contains("alice"), "{shown}");
    assert!(
        !shown.contains("hunter2") && !shown.contains("planted"),
        "{shown}"
    );
}

#[test]
fn the_acl_flags_set_what_the_broker_enforces_and_nothing_is_ignored() {
    let open = args(&[]).unwrap().settings().unwrap().broker;
    assert!(open.acl_default_allow && open.super_users.is_empty());
    let parsed = args(&["--acl-default", "deny", "--acl-super-user", "User:admin"]).unwrap();
    assert!(parsed.ignored().is_empty());
    let broker = parsed.settings().unwrap().broker;
    assert!(!broker.acl_default_allow);
    assert_eq!(broker.super_users.len(), 1);
    let allow = args(&["--acl-default", "allow"])
        .unwrap()
        .settings()
        .unwrap();
    assert!(allow.broker.acl_default_allow);
}

#[test]
fn a_bad_acl_default_or_super_user_is_refused_in_the_go_editions_words() {
    for bad in ["Allow", "", "yes", "deny "] {
        let message = args(&["--acl-default", bad]).unwrap_err().to_string();
        assert!(
            message.contains("must be \"allow\" or \"deny\""),
            "{bad:?}: {message}"
        );
    }
    for bad in ["admin", "User:", ":admin", "User:ok,alice", ":"] {
        let message = args(&["--acl-super-user", bad])
            .unwrap()
            .settings()
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("must be written Type:name"),
            "{bad:?}: {message}"
        );
    }
    // Blanks between commas are dropped, as Kafka drops them from super.users.
    let broker = args(&["--acl-super-user", "User:a, ,User:b,"])
        .unwrap()
        .settings()
        .unwrap()
        .broker;
    assert_eq!(broker.super_users.len(), 2);
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

#[test]
fn the_cleaner_looks_every_fifteen_seconds_unless_told_otherwise_and_a_zero_interval_is_refused() {
    let settings = args(&[]).unwrap().settings().unwrap();
    assert_eq!(
        (
            settings.broker.log_cleaner_every,
            settings.broker.log_cleaner_enable
        ),
        (std::time::Duration::from_secs(15), true)
    );
    let set = args(&[
        "--log-cleaner-interval",
        "2s",
        "--log-cleaner-enable",
        "false",
    ])
    .unwrap()
    .settings()
    .unwrap();
    assert_eq!(
        (set.broker.log_cleaner_every, set.broker.log_cleaner_enable),
        (std::time::Duration::from_secs(2), false)
    );
    let err = args(&["--log-cleaner-interval", "0s"])
        .unwrap()
        .settings()
        .unwrap_err();
    assert!(err.to_string().contains("--log-cleaner-interval"), "{err}");
}

#[test]
fn client_telemetry_is_on_unless_told_otherwise() {
    let on = |flags: &[&str]| {
        args(flags)
            .unwrap()
            .settings()
            .unwrap()
            .broker
            .client_telemetry
    };
    assert!(on(&[]));
    assert!(on(&["--client-telemetry"]));
    assert!(!on(&["--client-telemetry=false"]));
    assert!(!on(&["--client-telemetry", "false"]));
}

mod console;
mod users;

#[cfg(feature = "tls")]
mod tls;

#[test]
fn a_sasl_user_named_for_the_console_is_refused_by_name_and_never_says_its_password() {
    for flags in [
        &[
            "--sasl-mechanisms",
            "PLAIN",
            "--sasl-user",
            "__console:pw-planted",
        ][..],
        &[
            "--sasl-mechanisms",
            "SCRAM-SHA-256",
            "--sasl-user",
            "ok:x",
            "--sasl-user",
            "__console:pw-planted",
        ][..],
    ] {
        let said = sasl(flags).unwrap_err();
        assert!(
            said.contains("the user name __console is reserved for the web console"),
            "{said}"
        );
        assert!(!said.contains("planted"), "{said}");
    }
    // A name that only starts the same is fine.
    assert!(sasl(&["--sasl-mechanisms", "PLAIN", "--sasl-user", "__console2:pw"]).is_ok());
}
