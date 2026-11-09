use std::path::Path;

use kafka_meta::MetaStore;
use kafka_storage::sim::SimFs;
use kafka_wire::batch::build::BatchBuilder;
use kafka_wire::ids::{PartitionCount, PartitionId, TopicName};

use std::sync::Arc;

use kafka_meta::Topic;
use kafka_wire::batch::{Batch, TimestampType};
use kafka_wire::ids::TimestampMs;

use super::*;
use crate::Config;

fn setup() -> (Partitions<SimFs>, Arc<Topic>) {
    setup_with(|_| {}, 2)
}

fn setup_with(tweak: impl FnOnce(&mut Config), partitions: i32) -> (Partitions<SimFs>, Arc<Topic>) {
    let (partitions, topic, ..) = setup_full(tweak, partitions);
    (partitions, topic)
}

pub(super) fn setup_full(
    tweak: impl FnOnce(&mut Config),
    partitions: i32,
) -> (Partitions<SimFs>, Arc<Topic>, Arc<MetaStore>, SimFs) {
    let fs = SimFs::new();
    kafka_storage::fs::create_dirs_durably(&fs, Path::new("/data")).unwrap();
    let meta = Arc::new(MetaStore::open(&fs, Path::new("/data")).unwrap());
    let name = TopicName::parse("t").unwrap();
    let parts = PartitionCount::new(partitions).unwrap();
    let topic = meta
        .create_topic(&name, parts, Default::default(), &Default::default())
        .unwrap();
    kafka_storage::topic_dirs::create_partition_dirs(&fs, Path::new("/data"), &name, parts)
        .unwrap();
    let mut config = Config::new("/data".into());
    tweak(&mut config);
    (
        Partitions::new(fs.clone(), config, Default::default(), Arc::clone(&meta)),
        topic,
        meta,
        fs,
    )
}

#[tokio::test]
async fn a_topic_closed_leaves_no_partition_open_and_no_writer_that_takes_an_append() {
    let (partitions, topic) = setup();
    let id = PartitionId::new(0).unwrap();
    let first = partitions.get(&topic, id).await.unwrap();
    let again = partitions.get(&topic, id).await.unwrap();
    assert!(
        Arc::ptr_eq(&first, &again),
        "one open per partition, shared"
    );
    partitions
        .get(&topic, PartitionId::new(1).unwrap())
        .await
        .unwrap();
    assert_eq!(partitions.len(), 2);
    partitions.close_topic(topic.id).await;
    assert_eq!(partitions.len(), 0);
    let batch = Batch::parse(BatchBuilder::new().build()).unwrap();
    let late = first
        .submit(
            batch,
            TimestampMs::from_wire(1),
            TimestampType::Create,
            false,
        )
        .await;
    assert!(
        late.is_err(),
        "the writer is gone, and a late append is told so"
    );
}

#[tokio::test]
async fn past_the_limit_the_least_recently_used_idle_partition_is_closed_and_one_in_use_is_not() {
    let (partitions, topic) = setup_with(|c| c.max_open_partitions = 2, 4);
    let id = |n| PartitionId::new(n).unwrap();
    let held = partitions.get(&topic, id(0)).await.unwrap();
    for n in 1..4 {
        drop(partitions.get(&topic, id(n)).await.unwrap());
        assert!(
            partitions.len() <= 2,
            "after opening partition {n}: {} open",
            partitions.len()
        );
    }
    // Partition 0 was the oldest but something holds it; so it stayed, and the idle ones went.
    assert!(Arc::ptr_eq(
        &held,
        &partitions.get(&topic, id(0)).await.unwrap()
    ));
    // One that was closed opens again, and works.
    let again = partitions.get(&topic, id(1)).await.unwrap();
    assert_eq!(again.reader().end_offset().get(), 0);
}

#[tokio::test]
async fn a_request_that_knew_a_topic_before_it_was_deleted_and_made_again_does_not_open_it() {
    let (partitions, old, meta, fs) = setup_full(|_| {}, 2);
    // As DeleteTopics does it, the metadata first; then a topic of the same name is made.
    meta.delete_topic(&old.name).unwrap();
    let parts = PartitionCount::new(2).unwrap();
    let new = meta
        .create_topic(&old.name, parts, Default::default(), &Default::default())
        .unwrap();
    let id = PartitionId::new(1).unwrap();
    // The old topic's directory is the new topic's: opening it again for the old one would write through a second log.
    let before = fs.ops();
    assert!(partitions.get(&old, id).await.is_err());
    assert_eq!(
        fs.ops(),
        before,
        "no file was touched: opening a log runs recovery on what is there"
    );
    assert_eq!(partitions.len(), 0, "and nothing is left open for it");
    let served = partitions.get(&new, id).await.unwrap();
    assert_eq!(partitions.len(), 1);
    drop(served);
}

#[tokio::test]
async fn a_partition_being_closed_is_not_opened_again_until_it_is_closed() {
    let (partitions, topic) = setup();
    let id = PartitionId::new(0).unwrap();
    let open = partitions.get(&topic, id).await.unwrap();
    drop(open);
    let closing = partitions.pretend_closing((topic.id, id));
    let wait = std::time::Duration::from_millis(50);
    assert!(
        tokio::time::timeout(wait, partitions.get(&topic, id))
            .await
            .is_err(),
        "two logs on the same files would be worse than waiting"
    );
    // Another partition of the topic is not held up.
    partitions
        .get(&topic, PartitionId::new(1).unwrap())
        .await
        .unwrap();
    let (got, ()) = tokio::join!(partitions.get(&topic, id), async {
        tokio::time::sleep(wait).await;
        drop(closing);
    });
    assert!(got.is_ok(), "and once it is closed, it is opened");
}

mod txns;
