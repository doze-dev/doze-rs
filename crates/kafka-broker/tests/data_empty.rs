//! Empty a topic (docs/port/data-control.md): every record gone and the disk freed now; the topic, its configs and the groups'
//! committed offsets stay, and each partition carries on from its end. Refused while a transaction is under way on it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_broker::DataRefusal;
use support::acls::{admin, closed_world, grant, on, user};
use support::{caller, fetch_request, open_on_with, records, rig, topic, with_config};

const OFFSET_OUT_OF_RANGE: i16 = 1;
const DELETE: i8 = 6;

fn bytes_of(rig: &support::Rig, name: &str) -> u64 {
    let usage = rig.broker.data_usage(&caller()).unwrap();
    usage.topics.iter().find(|t| t.name == name).unwrap().bytes
}

#[tokio::test]
async fn emptying_frees_the_disk_keeps_the_topic_and_the_commits_and_carries_on_from_the_end() {
    let rig = rig();
    rig.create(vec![topic("t", 2)], false).await;
    for _ in 0..5 {
        rig.produce(-1, "t", 0, records(10)).await;
    }
    rig.produce(-1, "t", 1, records(3)).await;
    let r = rig
        .commit_offsets(2, "g", "", -1, &[("t", 0, 20, None)])
        .await;
    assert_eq!(r.topics[0].partitions[0].error_code, 0);
    let before = bytes_of(&rig, "t");
    assert!(before > 0);

    let done = rig.broker.empty_topic(&caller(), "t").await.unwrap();
    assert_eq!(
        (done.topic.as_str(), done.partitions, done.offsets),
        ("t", 2, 53)
    );
    // Every byte the records took is free; what is left is the record of where each partition starts now.
    let after = bytes_of(&rig, "t");
    assert_eq!(done.freed_bytes, before - after);
    assert_eq!(after, "50\n".len() as u64 + "3\n".len() as u64);
    // Earliest and latest are both the old end; the commit is as it was.
    assert_eq!(rig.list_offset(7, "t", 0, -2).await.offset, 50);
    assert_eq!(rig.list_offset(7, "t", 0, -1).await.offset, 50);
    assert_eq!(rig.list_offset(7, "t", 1, -2).await.offset, 3);
    let committed = rig.fetch_offsets(false, "g", None).await;
    assert_eq!(committed, vec![("t".to_owned(), 0, 20, String::new())]);
    // A reader at the commit is told it is out of range, as after retention; the next record takes the next offset.
    let below = rig
        .fetch(fetch_request(&[("t", 0, 20, 1 << 20)], 0, 1, 1 << 20))
        .await;
    assert_eq!(below[0].error_code, OFFSET_OUT_OF_RANGE);
    assert_eq!(rig.produce(-1, "t", 0, records(1)).await.base_offset, 50);
    // Emptying what is empty does nothing and says so.
    rig.broker.empty_topic(&caller(), "t").await.unwrap();
    let again = rig.broker.empty_topic(&caller(), "t").await.unwrap();
    assert_eq!((again.offsets, again.freed_bytes), (0, 0));
}

#[tokio::test]
async fn a_topic_that_only_compacts_is_emptied_too_and_its_configs_stay() {
    let rig = rig();
    rig.create(
        vec![with_config(topic("c", 1), "cleanup.policy", "compact")],
        false,
    )
    .await;
    rig.produce(
        -1,
        "c",
        0,
        support::keyed(&[("k", Some("1")), ("j", Some("2"))]),
    )
    .await;
    let done = rig.broker.empty_topic(&caller(), "c").await.unwrap();
    assert_eq!(done.offsets, 2);
    assert_eq!(rig.list_offset(7, "c", 0, -2).await.offset, 2);
    let usage = rig.broker.data_usage(&caller()).unwrap();
    let c = usage.topics.iter().find(|t| t.name == "c").unwrap();
    assert!(c.keeps.compact && !c.keeps.delete, "{:?}", c.keeps);
}

#[tokio::test]
async fn a_partition_never_written_to_is_left_alone_and_a_missing_topic_is_said() {
    let rig = rig();
    rig.create(vec![topic("t", 3)], false).await;
    rig.produce(-1, "t", 1, records(4)).await;
    let done = rig.broker.empty_topic(&caller(), "t").await.unwrap();
    assert_eq!((done.partitions, done.offsets), (1, 4));
    let missing = rig.broker.empty_topic(&caller(), "nope").await.unwrap_err();
    assert_eq!(
        missing,
        DataRefusal::NoTopic {
            topic: "nope".into()
        }
    );
}

#[tokio::test]
async fn a_transaction_under_way_refuses_and_touches_nothing_until_it_ends() {
    let rig = rig();
    rig.create(vec![topic("t", 1), topic("other", 1)], false)
        .await;
    rig.produce(-1, "t", 0, records(5)).await;
    let txn = rig.init_txn("tx-1", 60_000).await.unwrap();
    assert_eq!(rig.add_partitions("tx-1", txn, "t", &[0]).await, vec![0]);
    assert_eq!(
        rig.produce_txn("tx-1", txn, ("t", 0), 0, 2)
            .await
            .error_code,
        0
    );
    let refused = rig.broker.empty_topic(&caller(), "t").await.unwrap_err();
    assert_eq!(
        refused,
        DataRefusal::TxnOpen {
            topic: Some("t".into()),
            id: Some("tx-1".into())
        }
    );
    assert!(
        refused.sentence().contains("commit or abort it"),
        "{}",
        refused.sentence()
    );
    assert_eq!(
        rig.list_offset(7, "t", 0, -2).await.offset,
        0,
        "nothing was touched"
    );
    // Another topic is not in the way, but emptying every topic is.
    rig.broker.empty_topic(&caller(), "other").await.unwrap();
    assert!(matches!(
        rig.broker.empty_topics(&caller()).await,
        Err(DataRefusal::TxnOpen { topic: None, .. })
    ));
    assert_eq!(rig.end_txn("tx-1", txn, true).await, 0);
    // The markers are written when the transaction ends; then the topic can be emptied, past the marker.
    let done = rig.broker.empty_topic(&caller(), "t").await.unwrap();
    assert_eq!(
        done.offsets, 8,
        "5 records, 2 in the transaction, its marker"
    );
}

#[tokio::test]
async fn emptying_asks_delete_on_the_topic_and_every_topic_asks_alter_on_the_cluster() {
    let rig = open_on_with(kafka_storage::sim::SimFs::new(), closed_world);
    let mut req = kafka_wire::messages::CreateTopicsRequest::default();
    req.topics = vec![topic("t", 1)];
    rig.ask_as(&admin(), 7, kafka_wire::api::Request::CreateTopics(req))
        .await;
    let bob = user("bob");
    let refused = rig.broker.empty_topic(&bob, "t").await.unwrap_err();
    assert!(
        matches!(refused, DataRefusal::Denied { cluster: false, .. }),
        "{refused:?}"
    );
    assert_eq!(
        refused.error(),
        kafka_wire::ResponseError::TopicAuthorizationFailed
    );
    // A topic bob may not see is refused the same way as one that is not there: nothing is given away.
    let hidden = rig.broker.empty_topic(&bob, "nope").await.unwrap_err();
    assert!(matches!(hidden, DataRefusal::Denied { .. }));
    grant(&rig, vec![on("t", "User:bob", DELETE)]).await;
    rig.broker.empty_topic(&bob, "t").await.unwrap();
    let all = rig.broker.empty_topics(&bob).await.unwrap_err();
    assert!(
        matches!(all, DataRefusal::Denied { cluster: true, .. }),
        "{all:?}"
    );
    assert!(rig.broker.erase_everything(&bob).await.is_err());
    assert!(
        rig.broker.data_usage(&bob).is_none(),
        "the usage needs DESCRIBE on the cluster"
    );
    rig.broker.empty_topics(&admin()).await.unwrap();
}

/// Producers keep writing while the topic is emptied: every append gets its own offset, none is handed out twice, and the
/// log ends where the last one put it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn producers_writing_while_the_topic_is_emptied_never_get_an_offset_twice() {
    let rig = std::sync::Arc::new(rig());
    rig.create(vec![topic("t", 1)], false).await;
    let writer = {
        let rig = std::sync::Arc::clone(&rig);
        tokio::spawn(async move {
            let mut bases = Vec::new();
            for _ in 0..60 {
                let r = rig.produce(1, "t", 0, records(5)).await;
                assert_eq!(r.error_code, 0);
                bases.push(r.base_offset);
            }
            bases
        })
    };
    for _ in 0..5 {
        rig.broker.empty_topic(&caller(), "t").await.unwrap();
        tokio::task::yield_now().await;
    }
    let bases = writer.await.unwrap();
    let want: Vec<i64> = (0..60).map(|i| i * 5).collect();
    assert_eq!(bases, want, "in order, five apart, none twice");
    assert_eq!(rig.list_offset(7, "t", 0, -1).await.offset, 300);
    let start = rig.list_offset(7, "t", 0, -2).await.offset;
    let read = rig
        .fetch(fetch_request(&[("t", 0, start, 1 << 20)], 0, 1, 1 << 20))
        .await;
    assert_eq!(read[0].error_code, 0);
    for base in support::batch_bases(read[0].records.as_ref().unwrap()) {
        assert!(
            base >= start,
            "a record below the start {start} was served: {base}"
        );
    }
}
