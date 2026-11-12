//! Metadata and DeleteTopics, and what a topic leaves on disk.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_storage::fs::Fs;
use kafka_storage::sim::{Cut, DataCut, NameCut};
use support::{DATA, open_on, rig, topic};

#[tokio::test]
async fn metadata_describes_this_broker_and_every_partition_of_every_topic() {
    let rig = rig();
    rig.create(vec![topic("b", 1), topic("a", 2)], false).await;
    let m = rig.metadata(12, None, false).await;
    let me = &m.brokers[0];
    assert_eq!((me.node_id.0, &*me.host, me.port), (1, "127.0.0.1", 4242));
    assert_eq!(
        (m.controller_id.0, m.cluster_id.as_deref()),
        (1, Some("doze-kafka"))
    );
    let names: Vec<_> = m
        .topics
        .iter()
        .map(|t| t.name.as_ref().unwrap().0.to_string())
        .collect();
    assert_eq!(names, ["a", "b"], "listed by name");
    let a = &m.topics[0];
    assert_eq!(a.partitions.len(), 2);
    let p = &a.partitions[1];
    assert_eq!(
        (p.partition_index, p.leader_id.0, p.leader_epoch),
        (1, 1, 0)
    );
    assert_eq!(
        (
            p.replica_nodes.iter().map(|b| b.0).collect::<Vec<_>>(),
            p.isr_nodes.iter().map(|b| b.0).collect::<Vec<_>>()
        ),
        (vec![1], vec![1])
    );
    assert!(!a.topic_id.is_nil());
}

#[tokio::test]
async fn naming_no_topics_means_none_from_v1_and_all_in_v0() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    assert!(rig.metadata(12, Some(&[]), false).await.topics.is_empty());
    assert_eq!(rig.metadata(0, Some(&[]), false).await.topics.len(), 1);
}

#[tokio::test]
async fn an_unknown_topic_is_unknown_and_one_asked_to_be_created_exists_for_the_next_request() {
    let rig = rig();
    // Asked without permission to create: unknown, and still not there.
    let m = rig.metadata(9, Some(&["made-by-metadata"]), false).await;
    assert_eq!(m.topics[0].error_code, 3);
    assert!(!rig.has_topic("made-by-metadata").await);
    // With it: Apache Kafka answers that it does not exist, and has made it by the next request.
    let m = rig.metadata(9, Some(&["made-by-metadata"]), true).await;
    assert_eq!(
        (m.topics[0].error_code, m.topics[0].partitions.len()),
        (3, 0)
    );
    assert!(rig.has_topic("made-by-metadata").await);
    assert!(rig.dirs().contains(&"made-by-metadata-0".to_owned()));
    let m = rig.metadata(9, Some(&["made-by-metadata"]), true).await;
    assert_eq!(
        (m.topics[0].error_code, m.topics[0].partitions.len()),
        (0, 1)
    );
    // Before v4 a client could not say, and Kafka always created.
    rig.metadata(3, Some(&["old-client"]), false).await;
    assert!(rig.has_topic("old-client").await);
}

#[tokio::test]
async fn a_name_that_can_never_be_a_topic_is_invalid_and_not_merely_unknown() {
    let rig = rig();
    for bad in ["", ".", "bad name", &"x".repeat(250)] {
        let m = rig.metadata(9, Some(&[bad]), true).await;
        assert_eq!(m.topics[0].error_code, 17, "{bad:?}");
    }
    assert!(
        rig.topic_names().await.is_empty(),
        "nothing is created for a name that cannot be a topic"
    );
}

#[tokio::test]
async fn deleting_a_topic_takes_its_registration_and_directories_and_frees_the_name_for_a_new_topic()
 {
    let rig = rig();
    let first = rig
        .create(vec![topic("t", 2), topic("keep", 1)], false)
        .await;
    let gone = rig.delete_by_name(&["t", "nothing-here", "bad/name"]).await;
    let seen: Vec<_> = gone
        .iter()
        .map(|r| (r.name.as_ref().unwrap().0.to_string(), r.error_code))
        .collect();
    assert_eq!(
        seen,
        [
            ("t".into(), 0),
            ("nothing-here".into(), 3),
            ("bad/name".into(), 3)
        ]
    );
    assert_eq!(gone[0].topic_id, first[0].topic_id);
    assert_eq!(rig.topic_names().await, ["keep"]);
    assert_eq!(rig.dirs(), ["keep-0"]);
    // A topic of the same name afterwards is another topic.
    let again = rig.create(vec![topic("t", 1)], false).await;
    assert_ne!(again[0].topic_id, first[0].topic_id);
}

#[tokio::test]
async fn a_topic_can_be_deleted_by_its_id() {
    let rig = rig();
    let made = rig.create(vec![topic("t", 1)], false).await;
    let unknown = rig.delete_by_id(kafka_wire::Uuid::from_u128(7)).await;
    assert_eq!(
        (unknown[0].error_code, unknown[0].name.is_none()),
        // UNKNOWN_TOPIC_ID, as Kafka's `deleteTopic` throws it (SEC-216); it was 3 before the audit.
        (100, true)
    );
    let gone = rig.delete_by_id(made[0].topic_id).await;
    assert_eq!(
        (
            gone[0].error_code,
            gone[0].name.as_ref().unwrap().0.to_string()
        ),
        (0, "t".to_owned())
    );
    assert!(rig.topic_names().await.is_empty());
}

#[tokio::test]
async fn a_topic_that_was_acknowledged_is_there_after_a_power_cut_with_its_directories() {
    let rig = rig();
    rig.create(vec![topic("t", 3)], false).await;
    let cut = Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 9,
    };
    let after = open_on(rig.fs.power_cut(cut));
    assert_eq!(after.topic_names().await, ["t"]);
    assert_eq!(after.dirs(), ["t-0", "t-1", "t-2"]);
}

#[tokio::test]
async fn a_directory_left_by_a_topic_that_no_longer_exists_is_not_adopted_by_a_new_one() {
    let rig = rig();
    let stale = std::path::Path::new(DATA).join("x-0");
    rig.fs.create_dir(&stale).unwrap();
    rig.fs
        .open_or_create(&stale.join("00000000000000000000.log"))
        .unwrap();
    rig.create(vec![topic("x", 1)], false).await;
    assert!(
        rig.fs.names(&stale).is_empty(),
        "the new topic starts empty"
    );
}

/// Cuts the power on `rig` three ways and says whether the topic "t" is listed on each reopening.
async fn listed_after_cuts(rig: &support::Rig, seed: u64) -> Vec<bool> {
    let cuts = [
        (DataCut::Drop, NameCut::Durable),
        (DataCut::Pages, NameCut::Random),
        (DataCut::Keep, NameCut::Volatile),
    ];
    let mut listed = Vec::new();
    for (data, names) in cuts {
        let after = open_on(rig.fs.power_cut(Cut { data, names, seed }));
        listed.push(after.has_topic("t").await);
    }
    listed
}

#[tokio::test]
async fn a_power_cut_after_any_file_operation_of_a_create_keeps_it_if_it_was_acknowledged() {
    let probe = rig();
    let opened = probe.fs.ops();
    probe.create(vec![topic("t", 2)], false).await;
    let total = probe.fs.ops() - opened;
    for n in 1..=total {
        let rig = rig();
        rig.fs.halt_after(n);
        let created = rig.create(vec![topic("t", 2)], false).await.remove(0);
        let listed = listed_after_cuts(&rig, n).await;
        if created.error_code == 0 {
            assert_eq!(
                listed, [true; 3],
                "operation {n}: a create that was acknowledged is there"
            );
        }
    }
}

#[tokio::test]
async fn a_power_cut_after_any_file_operation_of_a_delete_loses_it_only_if_it_was_not_acknowledged()
{
    let probe = rig();
    probe.create(vec![topic("t", 2)], false).await;
    let from = probe.fs.ops();
    probe.delete_by_name(&["t"]).await;
    let total = probe.fs.ops() - from;
    for n in 1..=total {
        let rig = rig();
        rig.create(vec![topic("t", 2)], false).await;
        rig.fs.halt_after(n);
        let deleted = rig.delete_by_name(&["t"]).await.remove(0);
        let listed = listed_after_cuts(&rig, n).await;
        if deleted.error_code == 0 {
            assert_eq!(
                listed, [false; 3],
                "operation {n}: a delete that was acknowledged is gone"
            );
        }
    }
}
