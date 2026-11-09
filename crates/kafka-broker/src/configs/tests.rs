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
