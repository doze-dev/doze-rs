//! The Kafka 4 profile with the unstable versions on (`Features::unstable`, doze-kafka's `--unstable-api-versions`) against
//! a real Apache Kafka 4.4.0 started with `unstable.api.versions.enable=true` (`data/api-versions-4.4.0-unstable.json`), and
//! the profile with them off against the same broker with its defaults (`data/api-versions.json`, major 4).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use kafka_wire::profile::{ApiSet, Features, KafkaMajor, Profile};

/// (key, min, max) of every API a recorded broker lists.
fn apis(v: &serde_json::Value) -> Vec<(i64, i64, i64)> {
    v.as_array()
        .expect("apis")
        .iter()
        .map(|a| {
            (
                a["key"].as_i64().unwrap(),
                a["min"].as_i64().unwrap(),
                a["max"].as_i64().unwrap(),
            )
        })
        .collect()
}

fn unstable_broker() -> Vec<(i64, i64, i64)> {
    let rec: serde_json::Value =
        serde_json::from_str(include_str!("data/api-versions-4.4.0-unstable.json")).unwrap();
    apis(&rec["apis"])
}

fn default_broker() -> Vec<(i64, i64, i64)> {
    let rec: serde_json::Value = serde_json::from_str(include_str!("data/api-versions.json")).unwrap();
    let four = rec["brokers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["major"] == 4)
        .unwrap();
    apis(&four["apis"])
}

/// As recorded: SASL listed, no telemetry reporter.
fn features(unstable: bool) -> Features {
    Features {
        sasl: true,
        telemetry: false,
        unstable,
        served: ApiSet::ALL,
    }
}

/// `(key, max)` of what the Kafka 4 profile offers.
fn ours(unstable: bool) -> Vec<(i64, i64)> {
    Profile::new(KafkaMajor::DEFAULT)
        .advertised(features(unstable))
        .iter()
        .map(|a| (i64::from(a.key as i16), i64::from(a.max)))
        .collect()
}

/// The setting changes InitProducerId alone, on the real broker and here.
#[test]
fn the_unstable_versions_are_init_producer_id_v6_alone_on_kafka_4_4_0_and_here() {
    let (on, off) = (unstable_broker(), default_broker());
    let changed: Vec<_> = on.iter().filter(|a| !off.contains(a)).collect();
    assert_eq!(changed, [&(22, 0, 6)]);
    assert_eq!(on.len(), off.len());
    let (ours_on, ours_off) = (ours(true), ours(false));
    let changed: Vec<_> = ours_on.iter().filter(|a| !ours_off.contains(a)).collect();
    assert_eq!(changed, [&(22, 6)]);
}

/// Produce, InitProducerId and TxnOffsetCommit are offered to the version the real broker of each setting offers.
#[test]
fn the_three_transaction_apis_are_offered_to_the_real_brokers_versions() {
    for (unstable, real) in [(false, default_broker()), (true, unstable_broker())] {
        let mine = ours(unstable);
        for key in [0, 22, 28] {
            let theirs = real.iter().find(|a| a.0 == key).unwrap().2;
            let max = mine.iter().find(|a| a.0 == key).unwrap().1;
            assert_eq!(max, theirs, "key {key}, unstable {unstable}");
        }
    }
}

/// With the unstable versions on, no API is offered past what the real broker so started lists.
#[test]
fn with_the_unstable_versions_nothing_is_offered_past_the_real_broker() {
    let real = unstable_broker();
    for a in Profile::new(KafkaMajor::DEFAULT).advertised(features(true)) {
        let key = i64::from(a.key as i16);
        let (_, min, max) = *real.iter().find(|t| t.0 == key).unwrap();
        assert!(
            i64::from(a.min) >= min && i64::from(a.max) <= max,
            "{:?} {}..{} is wider than the real {min}..{max}",
            a.key,
            a.min,
            a.max
        );
    }
}
