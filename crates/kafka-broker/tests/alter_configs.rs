//! IncrementalAlterConfigs and AlterConfigs on topics as Apache Kafka 4.3.1 does them (`list-config-append`, the Kafka source
//! for the refusals): what is set is what DescribeConfigs then says, a validate-only changes nothing, and the refusals
//! come in Kafka's order and words.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use support::{TOPIC_RESOURCE as TOPIC, rig, topic, with_config};

const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;
const INVALID_CONFIG: i16 = 40;
const INVALID_REQUEST: i16 = 42;
const SET: i8 = 0;
const DELETE: i8 = 1;
const APPEND: i8 = 2;
const SUBTRACT: i8 = 3;
const DYNAMIC_TOPIC: i8 = 1;
const DEFAULT: i8 = 5;

async fn value_and_source(rig: &support::Rig, topic: &str, key: &str) -> (String, i8) {
    let c = rig.config_of(TOPIC, topic, key).await.unwrap();
    (c.value.unwrap().to_string(), c.config_source)
}

fn ok() -> (i16, Option<String>) {
    (0, None)
}

fn refused(code: i16, message: &str) -> (i16, Option<String>) {
    (code, Some(message.to_owned()))
}

#[tokio::test]
async fn a_set_is_seen_by_describe_a_delete_gives_the_default_back_and_validate_only_changes_nothing()
 {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let set = |value| vec![(TOPIC, "t", vec![("retention.ms", SET, Some(value))])];
    assert_eq!(rig.incremental(1, true, &set("1000")).await, [ok()]);
    assert_eq!(
        value_and_source(&rig, "t", "retention.ms").await.1,
        4,
        "not changed by a validate-only"
    );
    assert_eq!(rig.incremental(1, false, &set("1000")).await, [ok()]);
    assert_eq!(
        value_and_source(&rig, "t", "retention.ms").await,
        ("1000".to_owned(), DYNAMIC_TOPIC)
    );
    let delete = vec![(TOPIC, "t", vec![("retention.ms", DELETE, None)])];
    assert_eq!(rig.incremental(0, false, &delete).await, [ok()]);
    assert_eq!(value_and_source(&rig, "t", "retention.ms").await.1, 4);
}

#[tokio::test]
async fn append_and_subtract_work_on_a_list_from_its_default_and_refuse_what_is_not_a_list() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let ops = |op, value| vec![(TOPIC, "t", vec![("cleanup.policy", op, Some(value))])];
    assert_eq!(
        rig.incremental(1, false, &ops(APPEND, "compact")).await,
        [ok()]
    );
    assert_eq!(
        value_and_source(&rig, "t", "cleanup.policy").await.0,
        "delete,compact"
    );
    assert_eq!(
        rig.incremental(1, false, &ops(SUBTRACT, "delete")).await,
        [ok()]
    );
    assert_eq!(
        value_and_source(&rig, "t", "cleanup.policy").await.0,
        "compact"
    );
    let not_a_list = vec![(TOPIC, "t", vec![("retention.ms", APPEND, Some("1"))])];
    assert_eq!(
        rig.incremental(1, false, &not_a_list).await,
        [refused(
            INVALID_CONFIG,
            "Can't APPEND to key retention.ms because its type is not LIST."
        )]
    );
}

#[tokio::test]
async fn the_refusals_come_in_kafkas_order_and_words() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let one = |ops: Vec<support::Op<'static>>| vec![(TOPIC, "t", ops)];
    assert_eq!(
        rig.incremental(
            1,
            false,
            &[
                (TOPIC, "t", vec![("retention.ms", SET, Some("1"))]),
                (TOPIC, "t", vec![])
            ]
        )
        .await,
        [
            refused(INVALID_REQUEST, "Each resource must appear at most once."),
            refused(INVALID_REQUEST, "Each resource must appear at most once."),
        ]
    );
    assert_eq!(
        rig.incremental(
            1,
            false,
            &one(vec![
                ("retention.ms", SET, Some("1")),
                ("retention.ms", SET, Some("2"))
            ])
        )
        .await,
        [refused(
            INVALID_REQUEST,
            "Error due to duplicate config keys"
        )]
    );
    assert_eq!(
        rig.incremental(
            1,
            false,
            &one(vec![("retention.ms", SET, None), ("segment.ms", SET, None)])
        )
        .await,
        [refused(
            INVALID_REQUEST,
            "Null value not supported for : retention.ms, segment.ms"
        )]
    );
}

#[tokio::test]
async fn the_values_are_judged_before_the_topic_is_looked_for() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let one = |ops: Vec<support::Op<'static>>| vec![(TOPIC, "t", ops)];
    assert_eq!(
        rig.incremental(1, false, &one(vec![("nope", SET, Some("1"))]))
            .await,
        [refused(INVALID_CONFIG, "Unknown topic config name: nope")]
    );
    let bad = rig
        .incremental(1, false, &one(vec![("retention.ms", SET, Some("abc"))]))
        .await;
    assert_eq!(bad[0].0, INVALID_CONFIG);
    assert!(
        bad[0]
            .1
            .as_ref()
            .unwrap()
            .starts_with("Invalid value abc for configuration retention.ms:")
    );
    assert_eq!(
        rig.incremental(1, false, &one(vec![("retention.ms", 9, Some("1"))]))
            .await,
        [refused(INVALID_REQUEST, "Unknown operations type 9")]
    );
    assert_eq!(
        rig.incremental(
            1,
            false,
            &one(vec![
                ("min.compaction.lag.ms", SET, Some("500")),
                ("max.compaction.lag.ms", SET, Some("100")),
            ])
        )
        .await,
        [refused(
            INVALID_CONFIG,
            "conflict topic config setting min.compaction.lag.ms (500) > max.compaction.lag.ms (100)"
        )]
    );
}

#[tokio::test]
async fn a_resource_that_is_not_there_or_not_taken_is_refused_and_the_others_still_go_through() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    // The configs are judged before the topic's absence is.
    assert_eq!(
        rig.incremental(
            1,
            false,
            &[(TOPIC, "ghost", vec![("nope", SET, Some("1"))])]
        )
        .await,
        [refused(INVALID_CONFIG, "Unknown topic config name: nope")]
    );
    assert_eq!(
        rig.incremental(
            1,
            false,
            &[(TOPIC, "ghost", vec![("retention.ms", SET, Some("1"))])]
        )
        .await,
        [refused(
            UNKNOWN_TOPIC_OR_PARTITION,
            "The topic 'ghost' does not exist."
        )]
    );
    assert_eq!(
        rig.incremental(1, false, &[(TOPIC, "", vec![])]).await,
        [refused(
            INVALID_REQUEST,
            "Default topic resources are not allowed."
        )]
    );
    assert_eq!(
        rig.incremental(1, false, &[(77, "x", vec![])]).await,
        [refused(INVALID_REQUEST, "Unknown resource type 77")]
    );
    // Each resource is answered in the order given, a refusal of one not stopping the rest.
    let mixed = rig
        .incremental(
            1,
            false,
            &[
                (TOPIC, "ghost", vec![("retention.ms", SET, Some("1"))]),
                (TOPIC, "t", vec![("retention.ms", SET, Some("7"))]),
            ],
        )
        .await;
    assert_eq!(
        (mixed[0].0, mixed[1].clone()),
        (UNKNOWN_TOPIC_OR_PARTITION, ok())
    );
    assert_eq!(value_and_source(&rig, "t", "retention.ms").await.0, "7");
}

#[tokio::test]
async fn the_legacy_alter_replaces_all_the_topic_has_and_refuses_a_null() {
    let rig = rig();
    rig.create(vec![with_config(topic("t", 1), "retention.ms", "5")], false)
        .await;
    assert_eq!(
        rig.alter(
            1,
            false,
            &[(TOPIC, "t", vec![("segment.ms", Some("9999"))])]
        )
        .await,
        [ok()]
    );
    assert_eq!(
        value_and_source(&rig, "t", "segment.ms").await,
        ("9999".to_owned(), DYNAMIC_TOPIC)
    );
    assert_eq!(
        value_and_source(&rig, "t", "retention.ms").await.1,
        4,
        "what the request did not name went back to the default"
    );
    assert_eq!(
        rig.alter(2, false, &[(TOPIC, "t", vec![("segment.ms", None)])])
            .await,
        [refused(
            INVALID_REQUEST,
            "Null value not supported for : segment.ms"
        )]
    );
    assert_eq!(
        rig.alter(0, true, &[(TOPIC, "t", vec![("segment.ms", Some("1"))])])
            .await,
        [ok()]
    );
    assert_eq!(value_and_source(&rig, "t", "segment.ms").await.0, "9999");
    let _ = DEFAULT;
}
