//! CreateTopics against what Apache Kafka 4.3.1 answered (`createtopics-validation`, `createtopics-collisions`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use support::{assigned, rig, topic, with_config};

const BAD_CHARS: &str =
    "contains one or more characters other than ASCII alphanumerics, '.', '_' and '-'";

/// Each step is put to one broker in turn, as the recording was: (the topic asked for, the code, the message
/// or None, the partitions answered).
#[tokio::test]
async fn each_request_is_answered_as_apache_kafka_answers_it() {
    let long = "b".repeat(250);
    let steps = vec![
        ("valid", topic("ct-valid", 3), 0, None, 3),
        ("already-exists", topic("ct-valid", 3), 36, Some("Topic 'ct-valid' already exists.".to_owned()), -1),
        ("empty-name", topic("", 1), 17, Some("Topic name is invalid: the empty string is not allowed".to_owned()), -1),
        ("dot", topic(".", 1), 17, Some("Topic name is invalid: '.' is not allowed".to_owned()), -1),
        ("dot-dot", topic("..", 1), 17, Some("Topic name is invalid: '..' is not allowed".to_owned()), -1),
        ("name-249", topic(&"a".repeat(249), 1), 0, None, 1),
        ("name-250", topic(&long, 1), 17, Some(format!("Topic name is invalid: the length of '{long}' is longer than the max allowed length 249")), -1),
        ("illegal-space", topic("bad name", 1), 17, Some(format!("Topic name is invalid: 'bad name' {BAD_CHARS}")), -1),
        ("illegal-slash", topic("bad/name", 1), 17, Some(format!("Topic name is invalid: 'bad/name' {BAD_CHARS}")), -1),
        ("illegal-unicode", topic("café", 1), 17, Some(format!("Topic name is invalid: 'café' {BAD_CHARS}")), -1),
        ("legal-punctuation", topic("ok.name-with_all.3", 1), 0, None, 1),
        ("collision-first", topic("col.lide", 1), 0, None, 1),
        ("collision-second", topic("col_lide", 1), 17, Some("Topic 'col_lide' collides with existing topic: col.lide".to_owned()), -1),
        ("zero-partitions", topic("ct-zero", 0), 37, Some("Number of partitions was set to an invalid non-positive value.".to_owned()), -1),
        ("negative-partitions", topic("ct-neg", -2), 37, Some("Number of partitions was set to an invalid non-positive value.".to_owned()), -1),
        ("broker-defaults", { let mut t = topic("ct-defaults", -1); t.replication_factor = -1; t }, 0, None, 1),
        ("assignment-only", assigned("ct-assigned", &[(0, &[1]), (1, &[1])]), 0, None, 2),
        ("assignment-gap", assigned("ct-gap", &[(5, &[1])]), 39, Some("partitions should be a consecutive 0-based integer sequence".to_owned()), -1),
        ("assignment-unknown-broker", assigned("ct-badnode", &[(0, &[42])]), 39, Some("The manual partition assignment includes broker 42, but no such broker is registered.".to_owned()), -1),
        ("config-unknown-name", with_config(topic("ct-cfg1", 1), "no.such.config", "1"), 40, Some("Unknown topic config name: no.such.config".to_owned()), -1),
        ("config-valid-set", with_config(with_config(topic("ct-cfg9", 1), "retention.ms", "3600000"), "cleanup.policy", "compact,delete"), 0, None, 1),
    ];
    let rig = rig();
    for (label, asked, code, message, partitions) in steps {
        let got = rig.create(vec![asked], false).await;
        assert_eq!(got.len(), 1, "{label}");
        let r = &got[0];
        assert_eq!(r.error_code, code, "{label}: {:?}", r.error_message);
        assert_eq!(
            r.error_message.as_deref().map(str::to_owned),
            message,
            "{label}"
        );
        assert_eq!(r.num_partitions, partitions, "{label}");
        assert_eq!(
            r.replication_factor,
            if code == 0 { 1 } else { -1 },
            "{label}"
        );
        assert_eq!(
            r.topic_id.is_nil(),
            code != 0,
            "{label}: a topic id only for a topic made"
        );
        assert!(r.configs.is_some(), "{label}: configs are never null");
    }
}

#[tokio::test]
async fn a_manual_assignment_cannot_also_say_how_many_partitions_or_replicas() {
    let rig = rig();
    let mut both = assigned("ct-both", &[(0, &[1])]);
    both.num_partitions = 2;
    both.replication_factor = 1;
    let got = rig.create(vec![both], false).await;
    assert_eq!(got[0].error_code, 42);
    assert_eq!(
        got[0].error_message.as_deref(),
        Some(
            "A manual partition assignment was specified, but replication factor was not set to -1."
        )
    );
}

#[tokio::test]
async fn a_name_listed_twice_is_refused_once_and_neither_is_created_and_the_rest_are_not_held_up() {
    let rig = rig();
    let got = rig
        .create(
            vec![
                topic("ct-mix-ok", 1),
                topic("ct-dup", 1),
                topic("", 1),
                topic("ct-dup", 1),
                topic("ct-mix-ok2", 0),
            ],
            false,
        )
        .await;
    let seen: Vec<_> = got
        .iter()
        .map(|r| (r.name.0.to_string(), r.error_code))
        .collect();
    assert_eq!(
        seen,
        [
            ("ct-mix-ok".into(), 0),
            ("ct-dup".into(), 42),
            ("".into(), 17),
            ("ct-mix-ok2".into(), 37)
        ]
    );
    assert_eq!(
        got[1].error_message.as_deref(),
        Some("Duplicate topic name.")
    );
    assert_eq!(
        rig.dirs(),
        ["ct-mix-ok-0"],
        "only the one that was good has a directory"
    );
}

#[tokio::test]
async fn names_of_one_request_are_held_against_the_topics_before_it_for_dot_and_underscore_only() {
    let rig = rig();
    // Both made: Kafka checks each name of a request against the topics that existed before the request.
    let got = rig
        .create(vec![topic("a.b", 1), topic("a_b", 1)], false)
        .await;
    assert_eq!(got.iter().map(|r| r.error_code).collect::<Vec<_>>(), [0, 0]);
    // Not so for case: this broker refuses a name that differs only in case (a case-insensitive file system
    // would give the two one directory), where Apache Kafka accepts both.
    let got = rig
        .create(vec![topic("Case.Topic", 1), topic("case.topic", 1)], false)
        .await;
    assert_eq!(
        got.iter().map(|r| r.error_code).collect::<Vec<_>>(),
        [0, 17]
    );
}

#[tokio::test]
async fn a_validation_makes_nothing_but_answers_as_if_it_had() {
    let rig = rig();
    let got = rig
        .create(
            vec![topic("ct-validate", 2), topic("ct-validate2", 0)],
            true,
        )
        .await;
    assert_eq!(
        (
            got[0].error_code,
            got[0].num_partitions,
            got[0].topic_id.is_nil()
        ),
        (0, 2, false)
    );
    assert_eq!(got[1].error_code, 37);
    assert!(rig.dirs().is_empty());
    assert!(
        rig.create(vec![topic("ct-validate", 2)], false).await[0].error_code == 0,
        "the name is still free"
    );
}

#[tokio::test]
async fn the_configuration_in_force_is_answered_with_what_the_topic_set_marked_as_its_own() {
    let rig = rig();
    let got = rig
        .create(
            vec![with_config(topic("t", 1), "retention.ms", "3600000")],
            false,
        )
        .await;
    let configs = got[0].configs.as_ref().unwrap();
    let find = |name: &str| {
        configs
            .iter()
            .find(|c| &*c.name == name)
            .map(|c| (c.value.as_deref().unwrap().to_owned(), c.config_source))
    };
    assert_eq!(find("retention.ms"), Some(("3600000".into(), 1)));
    assert_eq!(find("cleanup.policy"), Some(("delete".into(), 5)));
}

#[tokio::test]
async fn a_topic_is_registered_only_if_its_partition_directories_could_be_made() {
    let setup = || {
        let rig = rig();
        rig.fs.clear_faults();
        rig
    };
    let cost = {
        let rig = setup();
        let from = rig.fs.ops();
        assert_eq!(
            rig.create(vec![topic("t", 3)], false).await[0].error_code,
            0
        );
        rig.fs.ops() - from
    };
    let mut failed_somewhere = 0;
    for n in 1..=cost {
        let rig = setup();
        rig.fs.fail_any(n, kafka_storage::sim::Failure::Io);
        let r = rig.create(vec![topic("t", 3)], false).await.remove(0);
        let metadata_says = rig.has_topic("t").await;
        if r.error_code == 0 {
            assert!(metadata_says, "operation {n}: answered as made");
        } else {
            failed_somewhere += 1;
            assert_eq!(r.error_code, 56, "operation {n}: {:?}", r.error_message);
            assert!(!metadata_says, "operation {n}: refused, so not registered");
            assert!(
                rig.dirs().iter().all(|d| !d.starts_with("t-")),
                "operation {n}: nothing left on disk: {:?}",
                rig.dirs()
            );
        }
    }
    assert!(
        failed_somewhere > 3,
        "most of a create's file operations are ones whose failure matters"
    );
}

#[tokio::test]
async fn a_topic_may_have_the_limit_of_partitions_and_no_more() {
    let rig = support::rig_limited(2);
    let got = rig
        .create(vec![topic("two", 2), topic("three", 3)], false)
        .await;
    assert_eq!((got[0].error_code, got[0].num_partitions), (0, 2));
    assert_eq!(got[1].error_code, 37);
    assert_eq!(
        got[1].error_message.as_deref(),
        Some("3 partitions is above this broker's limit of 2 (--max-partitions)")
    );
    assert_eq!(rig.dirs(), ["two-0", "two-1"]);
}
