use super::*;

fn set(name: &str, value: &str) -> Change {
    Change {
        name: name.into(),
        op: Op::Set,
        value: Some(value.into()),
    }
}

fn refused(changes: &[Change]) -> (E, String) {
    let r = apply("g", &BTreeMap::new(), changes, false).unwrap_err();
    (r.code, r.message)
}

#[test]
fn a_value_is_checked_by_kind_then_against_the_brokers_bounds_in_kafkas_words() {
    assert_eq!(
        refused(&[set(DELIVERY, "x")]),
        (E::InvalidConfig, "Invalid value x for configuration share.delivery.count.limit: Not a number of type INT".into())
    );
    assert_eq!(
        refused(&[set(DELIVERY, "1")]).1,
        "Invalid value 1 for configuration share.delivery.count.limit: Value must be at least 2"
    );
    assert_eq!(
        refused(&[set(DELIVERY, "11")]).1,
        "share.delivery.count.limit must be in the range 2 to 10 inclusive."
    );
    assert_eq!(
        refused(&[set(LOCK, "10000")]).1,
        "share.record.lock.duration.ms must be in the range 15000 to 60000 inclusive."
    );
    assert_eq!(
        refused(&[set(ISOLATION, "READ_COMMITTED")]).1,
        "Invalid value READ_COMMITTED for configuration share.isolation.level: String must be one of: read_committed, read_uncommitted"
    );
    assert_eq!(
        refused(&[set("nope", "1")]).1,
        "Unknown group config name: nope"
    );
    assert!(
        refused(&[set("consumer.session.timeout.ms", "50000")])
            .1
            .contains("not supported")
    );
    assert_eq!(
        apply("", &BTreeMap::new(), &[], false).unwrap_err().message,
        "Default group resources are not allowed."
    );
}

#[test]
fn the_reset_strategy_takes_kafkas_three_forms() {
    assert_eq!(parse_reset("earliest"), Some(Reset::Earliest));
    assert_eq!(parse_reset("LATEST"), Some(Reset::Latest));
    assert_eq!(
        parse_reset("by_duration:PT1H"),
        Some(Reset::ByDuration(3_600_000))
    );
    assert_eq!(
        parse_reset("by_duration:P1DT30M"),
        Some(Reset::ByDuration(86_400_000 + 1_800_000))
    );
    assert_eq!(
        parse_reset("by_duration:PT0.5S"),
        Some(Reset::ByDuration(500))
    );
    for bad in [
        "none",
        "by_duration:",
        "by_duration:PT-1H",
        "by_duration:1H",
        "by_duration:PT",
    ] {
        assert_eq!(parse_reset(bad), None, "{bad}");
    }
    assert!(
        refused(&[set(RESET, "none")])
            .1
            .starts_with("Invalid value `none` for configuration share.auto.offset.reset.")
    );
}

#[test]
fn a_session_must_be_longer_than_the_heartbeat() {
    let r = apply(
        "g",
        &BTreeMap::new(),
        &[set(SESSION, "45000"), set(HEARTBEAT, "15000")],
        false,
    );
    assert!(r.is_ok());
    let settings = ShareSettings::of(&r.unwrap());
    assert_eq!(
        (settings.session_ms, settings.heartbeat_ms),
        (45_000, 15_000)
    );
    assert_eq!(
        ShareSettings::of(&BTreeMap::new()),
        ShareSettings::default()
    );
}
