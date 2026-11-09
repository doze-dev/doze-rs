//! DescribeConfigs as Apache Kafka 4.3.1 answers it (`topic-config-keys`, `group-coordinator-defaults`): a topic's configs
//! in Kafka's order with where each comes from, the broker's, and the refusals.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use support::{BROKER, TOPIC_RESOURCE as TOPIC, rig, topic, with_config};

const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;
const INVALID_TOPIC_EXCEPTION: i16 = 17;
const INVALID_REQUEST: i16 = 42;
const DYNAMIC_TOPIC: i8 = 1;
const STATIC_BROKER: i8 = 4;
const DEFAULT: i8 = 5;

#[tokio::test]
async fn a_topic_lists_the_33_configs_kafka_lists_in_its_order_with_types_and_sources() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let r = rig
        .describe_configs(4, TOPIC, "t", None, false, false)
        .await;
    assert_eq!(r.error_code, 0);
    assert_eq!(r.configs.len(), 33);
    let names: Vec<&str> = r.configs.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names[0], "compression.type",
        "Kafka's order, not the alphabet's"
    );
    let by = |n: &str| r.configs.iter().find(|c| c.name.as_str() == n).unwrap();
    let cleanup = by("cleanup.policy");
    assert_eq!(cleanup.value.as_ref().unwrap().as_str(), "delete");
    assert_eq!((cleanup.config_source, cleanup.config_type), (DEFAULT, 7));
    assert!(!cleanup.read_only && !cleanup.is_sensitive);
    assert_eq!(by("retention.ms").config_type, 5);
    assert_eq!(
        by("flush.messages").value.as_ref().unwrap().as_str(),
        "9223372036854775807"
    );
    assert!(by("segment.bytes").documentation.is_none(), "not asked for");
    // The broker's flag differs from Kafka's default (128 MiB, not 1 GiB): the value is the broker's own setting.
    assert_eq!(by("segment.bytes").config_source, STATIC_BROKER);
}

#[tokio::test]
async fn naming_keys_gives_those_only_and_a_topic_own_value_is_a_dynamic_topic_config_with_its_synonyms()
 {
    let rig = rig();
    rig.create(
        vec![with_config(topic("t", 1), "retention.ms", "60000")],
        false,
    )
    .await;
    let r = rig
        .describe_configs(
            4,
            TOPIC,
            "t",
            Some(&["retention.ms", "cleanup.policy", "nope"]),
            true,
            true,
        )
        .await;
    assert_eq!(r.error_code, 0);
    let names: Vec<&str> = r.configs.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names.len(),
        2,
        "a key that is not a config is not an error, and not listed"
    );
    let retention = r
        .configs
        .iter()
        .find(|c| c.name.as_str() == "retention.ms")
        .unwrap();
    assert_eq!(retention.config_source, DYNAMIC_TOPIC);
    let synonyms: Vec<(String, String, i8)> = retention
        .synonyms
        .iter()
        .map(|s| {
            (
                s.name.to_string(),
                s.value.as_ref().unwrap().to_string(),
                s.source,
            )
        })
        .collect();
    assert_eq!(
        synonyms,
        [
            ("retention.ms".to_owned(), "60000".to_owned(), DYNAMIC_TOPIC),
            (
                "log.retention.ms".to_owned(),
                "-1".to_owned(),
                STATIC_BROKER
            ),
            ("log.retention.hours".to_owned(), "168".to_owned(), DEFAULT),
        ]
    );
    assert!(
        retention
            .documentation
            .as_ref()
            .is_some_and(|d| d.as_str().contains("retain"))
    );
    // An empty list of keys is all of them.
    let all = rig
        .describe_configs(4, TOPIC, "t", Some(&[]), false, false)
        .await;
    assert_eq!(all.configs.len(), 33);
}

#[tokio::test]
async fn a_topic_that_is_not_there_or_not_a_name_is_refused_as_kafka_refuses_it() {
    let rig = rig();
    let r = rig
        .describe_configs(4, TOPIC, "nope", None, false, false)
        .await;
    assert_eq!(r.error_code, UNKNOWN_TOPIC_OR_PARTITION);
    assert!(r.error_message.is_none() && r.configs.is_empty());
    let r = rig
        .describe_configs(4, TOPIC, "a b", None, false, false)
        .await;
    assert_eq!(r.error_code, INVALID_TOPIC_EXCEPTION);
    assert!(
        r.error_message
            .unwrap()
            .as_str()
            .starts_with("Topic name is invalid")
    );
}

#[tokio::test]
async fn the_broker_lists_its_own_configs_with_the_flags_it_was_started_with_and_kafkas_defaults() {
    let rig = rig();
    let r = rig
        .describe_configs(
            4,
            BROKER,
            "1",
            Some(&[
                "group.initial.rebalance.delay.ms",
                "offsets.retention.minutes",
            ]),
            true,
            false,
        )
        .await;
    assert_eq!(r.error_code, 0);
    let delay = r
        .configs
        .iter()
        .find(|c| c.name.as_str() == "group.initial.rebalance.delay.ms")
        .unwrap();
    assert_eq!(
        (delay.config_source, delay.read_only, delay.config_type),
        (STATIC_BROKER, true, 3)
    );
    assert_eq!(delay.value.as_ref().unwrap().as_str(), "200");
    let places: Vec<(i8, String)> = delay
        .synonyms
        .iter()
        .map(|s| (s.source, s.value.clone().unwrap().to_string()))
        .collect();
    assert_eq!(
        places,
        [
            (STATIC_BROKER, "200".to_owned()),
            (DEFAULT, "3000".to_owned())
        ]
    );
    let minutes = r
        .configs
        .iter()
        .find(|c| c.name.as_str() == "offsets.retention.minutes")
        .unwrap();
    assert_eq!(
        (
            minutes.config_source,
            minutes.value.as_ref().unwrap().as_str()
        ),
        (DEFAULT, "10080")
    );
    // What is set for all brokers is nothing, to start with.
    let all = rig
        .describe_configs(4, BROKER, "", None, false, false)
        .await;
    assert_eq!((all.error_code, all.configs.len()), (0, 0));
}

#[tokio::test]
async fn a_broker_that_is_not_this_one_and_a_type_not_taken_are_refused_in_kafkas_words() {
    let rig = rig();
    let r = rig
        .describe_configs(4, BROKER, "2", None, false, false)
        .await;
    assert_eq!(r.error_code, INVALID_REQUEST);
    assert_eq!(
        r.error_message.unwrap().as_str(),
        "Unexpected broker id, expected 1 or empty string, but received 2"
    );
    let r = rig
        .describe_configs(4, BROKER, "x", None, false, false)
        .await;
    assert_eq!(
        r.error_message.unwrap().as_str(),
        "Broker id must be an integer, but it is: x"
    );
    let r = rig.describe_configs(4, 8, "1", None, false, false).await;
    assert_eq!(r.error_code, INVALID_REQUEST);
    assert_eq!(
        r.error_message.unwrap().as_str(),
        "Unsupported resource type: BROKER_LOGGER"
    );
}

#[tokio::test]
async fn every_offered_version_answers() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    for v in 1..=4 {
        let r = rig.describe_configs(v, TOPIC, "t", None, true, true).await;
        assert_eq!((v, r.error_code, r.configs.len()), (v, 0, 33));
        let b = rig.describe_configs(v, BROKER, "1", None, true, true).await;
        assert_eq!((v, b.error_code), (v, 0));
    }
}
