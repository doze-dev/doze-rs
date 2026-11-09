//! The configs of the broker that AlterConfigs changes while it runs: kept across a restart, seen by DescribeConfigs
//! with their sources, and acted on by the topics that have not set their own.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use support::{BROKER, TOPIC_RESOURCE as TOPIC, open_on, records, rig, topic, with_config};

const MESSAGE_TOO_LARGE: i16 = 10;
const INVALID_REQUEST: i16 = 42;
const SET: i8 = 0;
const DELETE: i8 = 1;
const DYNAMIC_BROKER: i8 = 2;
const DYNAMIC_DEFAULT: i8 = 3;

fn set(name: &'static str, value: &'static str) -> support::Op<'static> {
    (name, SET, Some(value))
}

fn ok() -> (i16, Option<String>) {
    (0, None)
}

async fn broker_value(rig: &support::Rig, scope: &str, key: &str) -> Option<(String, i8)> {
    let r = rig
        .describe_configs(4, BROKER, scope, Some(&[key]), false, false)
        .await;
    r.configs
        .into_iter()
        .next()
        .map(|c| (c.value.unwrap().to_string(), c.config_source))
}

#[tokio::test]
async fn a_config_set_for_all_brokers_or_for_this_one_is_described_with_where_it_comes_from_and_this_ones_wins()
 {
    let rig = rig();
    assert_eq!(
        rig.incremental(
            1,
            false,
            &[(BROKER, "", vec![set("log.retention.bytes", "9000")])]
        )
        .await,
        [ok()]
    );
    assert_eq!(
        broker_value(&rig, "", "log.retention.bytes").await,
        Some(("9000".to_owned(), DYNAMIC_DEFAULT))
    );
    assert_eq!(
        broker_value(&rig, "1", "log.retention.bytes").await,
        Some(("9000".to_owned(), DYNAMIC_DEFAULT))
    );
    assert_eq!(
        rig.incremental(
            1,
            false,
            &[(BROKER, "1", vec![set("log.retention.bytes", "7000")])]
        )
        .await,
        [ok()]
    );
    let c = rig
        .config_of(BROKER, "1", "log.retention.bytes")
        .await
        .unwrap();
    assert_eq!(
        (c.value.unwrap().as_str(), c.config_source),
        ("7000", DYNAMIC_BROKER)
    );
    let places: Vec<(i8, String)> = c
        .synonyms
        .iter()
        .map(|s| (s.source, s.value.clone().unwrap().to_string()))
        .collect();
    assert_eq!(
        &places[..2],
        [(2, "7000".to_owned()), (3, "9000".to_owned())]
    );
    // The topic that sets no retention.bytes of its own is given the broker's, and says so.
    rig.create(vec![topic("t", 1)], false).await;
    let t = rig.config_of(TOPIC, "t", "retention.bytes").await.unwrap();
    assert_eq!(
        (t.value.unwrap().as_str(), t.config_source),
        ("7000", DYNAMIC_BROKER)
    );
    // Deleting this broker's own gives the cluster's back.
    assert_eq!(
        rig.incremental(
            1,
            false,
            &[(BROKER, "1", vec![("log.retention.bytes", DELETE, None)])]
        )
        .await,
        [ok()]
    );
    assert_eq!(
        broker_value(&rig, "1", "log.retention.bytes").await,
        Some(("9000".to_owned(), DYNAMIC_DEFAULT))
    );
}

#[tokio::test]
async fn what_cannot_be_changed_while_the_broker_runs_is_refused_in_kafkas_words() {
    let rig = rig();
    let one = |ops: Vec<support::Op<'static>>| vec![(BROKER, "1", ops)];
    let refused = |m: &str| vec![(INVALID_REQUEST, Some(m.to_owned()))];
    assert_eq!(
        rig.incremental(1, false, &one(vec![set("num.partitions", "3")]))
            .await,
        refused("Cannot update these configs dynamically: [num.partitions]")
    );
    assert_eq!(
        rig.incremental(1, false, &one(vec![set("foo.bar", "3")]))
            .await,
        refused("Unknown Dynamic Configuration: foo.bar.")
    );
    assert_eq!(
        rig.incremental(1, false, &one(vec![set("log.segment.bytes", "10")]))
            .await,
        refused(
            "Invalid value 10 for configuration log.segment.bytes: Value must be at least 1048576"
        )
    );
    assert_eq!(
        rig.incremental(1, false, &one(vec![set("message.max.bytes", "many")]))
            .await,
        refused("Invalid value many for configuration message.max.bytes: Not a number of type INT")
    );
    assert_eq!(
        rig.incremental(
            1,
            false,
            &[(BROKER, "2", vec![set("log.retention.ms", "5")])]
        )
        .await,
        refused("Unexpected broker id, expected 1, but received 2")
    );
    assert_eq!(
        rig.incremental(
            1,
            false,
            &[(BROKER, "one", vec![set("log.retention.ms", "5")])]
        )
        .await,
        refused("Node id must be an integer, but it is: one")
    );
    // Nothing of a refused request was kept; a validate-only keeps nothing either.
    assert_eq!(
        rig.incremental(1, true, &one(vec![set("log.retention.ms", "5")]))
            .await,
        [ok()]
    );
    assert_eq!(
        broker_value(&rig, "1", "log.retention.ms").await.unwrap().1,
        4
    );
}

#[tokio::test]
async fn the_legacy_alter_of_the_broker_replaces_what_was_set_there() {
    let rig = rig();
    rig.alter(
        1,
        false,
        &[(BROKER, "", vec![("log.retention.bytes", Some("1000"))])],
    )
    .await;
    rig.alter(
        1,
        false,
        &[(BROKER, "", vec![("log.retention.ms", Some("5000"))])],
    )
    .await;
    assert_eq!(broker_value(&rig, "", "log.retention.bytes").await, None);
    assert_eq!(
        broker_value(&rig, "", "log.retention.ms").await,
        Some(("5000".to_owned(), DYNAMIC_DEFAULT))
    );
}

#[tokio::test]
async fn the_configs_set_are_there_after_a_restart() {
    let rig = rig();
    rig.incremental(
        1,
        false,
        &[(BROKER, "", vec![set("log.retention.bytes", "9000")])],
    )
    .await;
    rig.incremental(
        1,
        false,
        &[(BROKER, "1", vec![set("message.max.bytes", "4096")])],
    )
    .await;
    let again = open_on(rig.fs.process_restart());
    assert_eq!(
        broker_value(&again, "1", "log.retention.bytes").await,
        Some(("9000".to_owned(), DYNAMIC_DEFAULT))
    );
    assert_eq!(
        broker_value(&again, "1", "message.max.bytes").await,
        Some(("4096".to_owned(), DYNAMIC_BROKER))
    );
}

#[tokio::test]
async fn a_larger_batch_than_the_brokers_limit_is_refused_from_the_moment_the_limit_is_set() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    assert_eq!(rig.produce(-1, "t", 0, records(10)).await.error_code, 0);
    rig.incremental(
        1,
        false,
        &[(BROKER, "", vec![set("message.max.bytes", "100")])],
    )
    .await;
    assert_eq!(
        rig.produce(-1, "t", 0, records(10)).await.error_code,
        MESSAGE_TOO_LARGE
    );
    // A topic with a limit of its own is not touched by it.
    rig.create(
        vec![with_config(topic("u", 1), "max.message.bytes", "1000000")],
        false,
    )
    .await;
    assert_eq!(rig.produce(-1, "u", 0, records(10)).await.error_code, 0);
}

#[tokio::test]
async fn a_topic_changed_while_open_rolls_and_is_trimmed_by_what_it_was_changed_to() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    for _ in 0..60 {
        assert_eq!(rig.produce(-1, "t", 0, records(10)).await.error_code, 0);
    }
    // One big segment so far: nothing for retention to take. The topic is changed, and what is written next rolls.
    let both = vec![set("segment.bytes", "1024"), set("retention.bytes", "5000")];
    assert_eq!(
        rig.incremental(1, false, &[(TOPIC, "t", both)]).await,
        [ok()]
    );
    for _ in 0..60 {
        assert_eq!(rig.produce(-1, "t", 0, records(10)).await.error_code, 0);
    }
    rig.broker.check_logs().await;
    let start = rig.list_offset(7, "t", 0, -2).await.offset;
    assert!(start > 0, "the start moved to {start}");
}

#[tokio::test]
async fn a_segment_size_set_for_the_broker_reaches_a_partition_that_is_already_open() {
    let rig = rig();
    rig.create(
        vec![with_config(topic("t", 1), "retention.bytes", "100000")],
        false,
    )
    .await;
    assert_eq!(rig.produce(-1, "t", 0, records(10)).await.error_code, 0);
    rig.incremental(
        1,
        false,
        &[(BROKER, "", vec![set("log.segment.bytes", "1048576")])],
    )
    .await;
    // Enough to roll the segment several times over at the new size (and not at the 128 MiB it was opened with).
    for _ in 0..20_000 {
        assert_eq!(rig.produce(-1, "t", 0, records(10)).await.error_code, 0);
    }
    rig.broker.check_logs().await;
    assert!(rig.list_offset(7, "t", 0, -2).await.offset > 0);
}
