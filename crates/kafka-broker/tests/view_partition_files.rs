//! `ConsoleView::partition_files`: the files of a topic's partitions, for the console's topic pages.
#![cfg(feature = "inspect")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_auth::Principal;
use kafka_broker::Caller;
use kafka_storage::sim::{Failure, Op};
use support::{records, rig, rig_with, topic};

fn console() -> Caller {
    Caller::new(Principal::console(), Some("127.0.0.1".parse().unwrap()))
}

#[tokio::test]
async fn each_partition_reports_its_size_and_a_partition_never_written_reports_none() {
    let rig = rig();
    rig.create(vec![topic("t", 3)], false).await;
    rig.produce(1, "t", 0, records(5)).await;
    rig.produce(1, "t", 0, records(5)).await;
    rig.produce(1, "t", 2, records(1)).await;
    let files = rig
        .broker
        .console_view()
        .partition_files(&console(), "t")
        .unwrap();
    assert_eq!(files.len(), 3);
    assert_eq!(files.iter().map(|f| f.partition).collect::<Vec<_>>(), [0, 1, 2]);
    assert!(files[0].bytes > files[2].bytes && files[2].bytes > 0, "{files:?}");
    assert_eq!((files[1].bytes, files[1].segments, files[1].last_write_ms), (0, 0, None));
    assert!(files[0].open && files[2].open && !files[1].open);
    assert!(files[0].last_write_ms.is_some());
    assert!(files.iter().all(|f| f.offline.is_none() && !f.unreadable));
}

#[tokio::test]
async fn a_partition_that_was_closed_to_make_room_reports_the_same_from_its_directory() {
    let rig = rig_with(|c| c.max_open_partitions = 1);
    rig.create(vec![topic("t", 2)], false).await;
    rig.produce(1, "t", 0, records(4)).await;
    let before = rig
        .broker
        .console_view()
        .partition_files(&console(), "t")
        .unwrap();
    assert!(before[0].open);
    // Partition 1 takes the one place; partition 0 is closed (and flushed).
    rig.produce(1, "t", 1, records(1)).await;
    let after = rig
        .broker
        .console_view()
        .partition_files(&console(), "t")
        .unwrap();
    assert!(!after[0].open && after[1].open, "{after:?}");
    assert_eq!(after[0].bytes, before[0].bytes);
    assert_eq!(after[0].segments, 1);
    // Looking at them opened nothing.
    assert_eq!(rig.broker.open_partitions(), 1);
}

#[tokio::test]
async fn a_partition_whose_flush_failed_is_offline_with_the_cause_and_the_others_are_not() {
    let rig = rig();
    rig.create(vec![topic("t", 2)], false).await;
    rig.produce(-1, "t", 0, records(1)).await;
    rig.produce(-1, "t", 1, records(1)).await;
    rig.fs.fail(Op::Fsync, 1, Failure::Io);
    let lost = rig.produce(-1, "t", 0, records(1)).await;
    assert_eq!(lost.error_code, 56);
    let files = rig
        .broker
        .console_view()
        .partition_files(&console(), "t")
        .unwrap();
    let cause = files[0].offline.as_deref().expect("partition 0 is offline");
    assert!(!cause.is_empty());
    assert_eq!(files[1].offline, None);
}

#[tokio::test]
async fn a_topic_the_caller_may_not_describe_or_that_does_not_exist_is_nothing() {
    let rig = rig_with(|c| c.acl_default_allow = false);
    let mut req = kafka_wire::messages::CreateTopicsRequest::default();
    req.topics = vec![topic("t", 1)];
    let made = rig
        .ask_as(&console(), 7, kafka_wire::api::Request::CreateTopics(req))
        .await;
    assert!(matches!(made, kafka_wire::api::Response::CreateTopics(r) if r.topics[0].error_code == 0));
    let view = rig.broker.console_view();
    let nobody = Caller::anonymous(Some("127.0.0.1".parse().unwrap()));
    assert!(view.partition_files(&nobody, "t").is_none());
    assert!(view.partition_files(&console(), "t").is_some(), "the console is a super user");
    assert!(view.partition_files(&console(), "absent").is_none());
    assert!(view.partition_files(&console(), "bad name!").is_none());
}
