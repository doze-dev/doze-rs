use super::*;

fn set(name: &str, value: &str) -> Change {
    Change {
        name: name.into(),
        op: Op::Set,
        value: Some(value.into()),
    }
}

fn change(name: &str, op: Op, value: Option<&str>) -> Change {
    Change {
        name: name.into(),
        op,
        value: value.map(Into::into),
    }
}

fn own(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).into(), (*v).into()))
        .collect()
}

fn run(current: &[(&str, &str)], changes: &[Change]) -> Result<BTreeMap<String, String>, Refusal> {
    apply(&Defaults::default(), &own(current), changes)
}

#[test]
fn a_set_replaces_a_delete_removes_and_deleting_what_is_not_set_is_fine() {
    let out = run(
        &[("retention.ms", "5"), ("segment.ms", "9")],
        &[
            set("retention.ms", "7"),
            change("segment.ms", Op::Delete, None),
            change("flush.ms", Op::Delete, None),
        ],
    )
    .unwrap();
    assert_eq!(out, own(&[("retention.ms", "7")]));
}

#[test]
fn an_append_starts_from_the_default_when_the_topic_has_none_and_adds_only_what_is_missing() {
    let append = |v| change("cleanup.policy", Op::Append, Some(v));
    assert_eq!(
        run(&[], &[append("compact")]).unwrap(),
        own(&[("cleanup.policy", "delete,compact")])
    );
    assert_eq!(
        run(&[("cleanup.policy", "compact")], &[append("compact")]).unwrap(),
        own(&[("cleanup.policy", "compact")]),
        "already there"
    );
    let out = run(
        &[("cleanup.policy", "compact,delete")],
        &[change("cleanup.policy", Op::Subtract, Some("delete"))],
    )
    .unwrap();
    assert_eq!(out, own(&[("cleanup.policy", "compact")]));
}

#[test]
fn an_append_or_subtract_on_what_is_not_a_list_is_invalid_config_in_kafkas_words() {
    for (op, word) in [(Op::Append, "APPEND"), (Op::Subtract, "SUBTRACT")] {
        let refused = run(&[], &[change("retention.ms", op, Some("1"))]).unwrap_err();
        assert_eq!(refused.code, E::InvalidConfig);
        assert_eq!(
            refused.message,
            format!("Can't {word} to key retention.ms because its type is not LIST.")
        );
    }
}

#[test]
fn the_requests_own_faults_come_before_the_configs_and_name_what_is_wrong() {
    let twice = run(&[], &[set("retention.ms", "1"), set("retention.ms", "2")]).unwrap_err();
    assert_eq!(
        (twice.code, twice.message.as_str()),
        (E::InvalidRequest, "Error due to duplicate config keys")
    );
    let none = run(
        &[],
        &[
            change("retention.ms", Op::Set, None),
            change("segment.ms", Op::Set, None),
        ],
    )
    .unwrap_err();
    assert_eq!(
        (none.code, none.message.as_str()),
        (
            E::InvalidRequest,
            "Null value not supported for : retention.ms, segment.ms"
        )
    );
    let unknown = run(&[], &[set("nope", "1")]).unwrap_err();
    assert_eq!(
        (unknown.code, unknown.message.as_str()),
        (E::InvalidConfig, "Unknown topic config name: nope")
    );
    let bad = run(&[], &[set("retention.ms", "abc")]).unwrap_err();
    assert_eq!(bad.code, E::InvalidConfig);
    assert!(
        bad.message
            .starts_with("Invalid value abc for configuration retention.ms"),
        "{}",
        bad.message
    );
}

#[test]
fn a_value_of_the_longest_size_is_kept_and_one_byte_more_is_invalid_config() {
    // Kafka keeps a config value of up to Short.MAX_VALUE (32767) and refuses a longer one with INVALID_CONFIG
    // (`ConfigurationControlManager.DISALLOWED_CONFIG_VALUE_SIZE_ERROR`). A free-text config takes any value.
    let text = |n: usize| {
        run(
            &[],
            &[set("leader.replication.throttled.replicas", &"1".repeat(n))],
        )
    };
    assert!(text(32_767).is_ok(), "the longest is fine");
    let refused = text(32_768).unwrap_err();
    assert_eq!(refused.code, E::InvalidConfig);
    assert!(refused.message.contains("32767"), "{}", refused.message);
}

fn broker_run(
    current: &[(&str, &str)],
    changes: &[Change],
) -> Result<BTreeMap<String, String>, Refusal> {
    apply_to_broker(&own(current), changes)
}

#[test]
fn a_broker_number_may_be_the_largest_its_type_holds_and_is_read_as_kafka_reads_numbers() {
    // An INT config takes up to 2^31-1, a LONG one up to 2^63-1; both ends are in. Kafka's ConfigDef trims a
    // number before it parses it (`ConfigDef.parseType`).
    for (name, value) in [
        ("message.max.bytes", "2147483647"),
        ("log.retention.ms", "9223372036854775807"),
        ("log.segment.bytes", " 2000000 "),
    ] {
        let got = broker_run(&[], &[set(name, value)]);
        assert_eq!(got.unwrap(), own(&[(name, value)]), "{name}");
    }
    let past = broker_run(&[], &[set("message.max.bytes", "2147483648")]).unwrap_err();
    assert_eq!(
        past.message,
        "Invalid value 2147483648 for configuration message.max.bytes: Not a number of type INT"
    );
}

#[test]
fn a_dynamic_value_that_is_not_a_number_leaves_the_default_in_force() {
    use crate::configs::Dynamic;
    let base = Defaults::default();
    let dynamic = Dynamic {
        cluster: own(&[
            ("log.retention.ms", "abc"),
            ("log.segment.bytes", "2000000"),
        ]),
        own: own(&[("message.max.bytes", "")]),
    };
    let got = broker::overlay(base, &dynamic);
    assert_eq!(got.retention_ms, base.retention_ms, "unreadable: skipped");
    assert_eq!(got.message_max_bytes, base.message_max_bytes);
    assert_eq!(got.segment_bytes, 2_000_000, "and the rest still applies");
}
