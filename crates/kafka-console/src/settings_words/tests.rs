use std::time::Duration;

use kafka_wire::profile::{ApiSet, Features, KafkaMajor, Profile};

use super::*;

#[test]
fn a_change_counts_when_the_broker_reads_the_config_and_the_sentence_says_how_often() {
    let cases = [
        (
            "log.segment.bytes",
            None,
            "At the next append of each open partition",
        ),
        (
            "log.index.interval.bytes",
            None,
            "At the next append of each open partition",
        ),
        ("message.max.bytes", None, "On the next request"),
        (
            "log.retention.bytes",
            Some(300),
            "At the next retention pass, every 5 m",
        ),
        (
            "log.retention.ms",
            Some(300),
            "At the next retention pass, every 5 m",
        ),
        ("log.retention.ms", None, "At the next retention pass"),
        ("num.partitions", None, "Not acted on"),
    ];
    for (key, pace, want) in cases {
        assert_eq!(takes_effect(key, pace), want, "{key}");
    }
}

#[test]
fn a_value_has_its_words() {
    let cases = [
        ("log.segment.bytes", "134217728", Some("128 MiB")),
        ("log.index.interval.bytes", "4096", Some("4 KiB")),
        ("message.max.bytes", "1048588", Some("1 MiB + 12 B")),
        ("log.retention.bytes", "-1", Some("no limit")),
        ("log.retention.ms", "-1", Some("no limit")),
        ("log.retention.ms", "259200000", Some("3 d")),
        ("log.retention.ms", "500", None),
        ("log.retention.ms", "many", None),
        ("num.partitions", "3", None),
    ];
    for (key, value, want) in cases {
        assert_eq!(gloss(key, value).as_deref(), want, "{key} {value}");
    }
    assert_eq!(
        unit("log.retention.ms", "259200000").as_deref(),
        Some("ms = 3 d")
    );
    assert_eq!(
        unit("log.segment.bytes", "134217728").as_deref(),
        Some("B = 128 MiB")
    );
    assert_eq!(unit("log.retention.ms", "x"), None);
}

#[test]
fn a_duration_is_written_as_the_flags_write_it() {
    let cases = [
        (300_000, "5m"),
        (15_000, "15s"),
        (500, "500ms"),
        (604_800_000, "168h"),
        (200, "200ms"),
        (45_000, "45s"),
        (0, "0"),
        (90_000, "90s"),
        (3_600_000, "1h"),
        (1_500, "1500ms"),
    ];
    for (ms, want) in cases {
        assert_eq!(flag_duration(Duration::from_millis(ms)), want, "{ms}");
    }
    assert_eq!(spoken(Duration::from_millis(500)), "500 ms");
    assert_eq!(spoken(Duration::from_secs(300)), "5 m");
    assert_eq!(spoken(Duration::from_secs(15)), "15 s");
}

#[test]
fn versions_say_only_when_there_is_one() {
    assert_eq!(versions(0, 0), "v0 only");
    assert_eq!(versions(3, 9), "v3 to v9");
}

#[test]
fn every_api_a_kafka_profile_offers_has_a_family_and_a_place_and_no_two_share_one() {
    let mut places = std::collections::BTreeSet::new();
    for major in KafkaMajor::RANGE {
        let profile = Profile::new(KafkaMajor::new(major).unwrap());
        let all = profile.advertised(Features {
            sasl: true,
            telemetry: true,
            served: ApiSet::ALL,
        });
        for a in all {
            let (place, family) = family_of(a.key)
                .unwrap_or_else(|| panic!("{:?} has no family (Kafka {major})", a.key));
            assert!(!family.is_empty());
            if major == 4 {
                assert!(places.insert(place), "two APIs share place {place}");
            }
        }
    }
}

#[test]
fn a_fixed_config_names_its_flag_where_it_has_one() {
    assert_eq!(flag_of("num.partitions"), Some("--default-partitions"));
    assert_eq!(
        flag_of("offsets.retention.minutes"),
        Some("--offsets-retention")
    );
    assert_eq!(flag_of("broker.id"), None);
    assert_eq!(flag_of("log.retention.ms"), None);
}

#[test]
fn a_default_is_seen_as_its_words() {
    assert_eq!(seen_as("log.retention.ms", "259200000"), "3 d");
    assert_eq!(seen_as("log.segment.bytes", "134217728"), "128 MiB");
    assert_eq!(seen_as("log.retention.ms", "-1"), "-1");
    assert_eq!(seen_as("log.retention.ms", "x"), "x");
}
