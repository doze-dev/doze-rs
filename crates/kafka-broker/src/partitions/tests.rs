use std::path::Path;

use kafka_meta::MetaStore;
use kafka_storage::sim::SimFs;
use kafka_wire::batch::build::BatchBuilder;
use kafka_wire::ids::{PartitionCount, PartitionId, TopicName};

use super::*;

fn setup() -> (Partitions<SimFs>, Arc<Topic>) {
    let fs = SimFs::new();
    kafka_storage::fs::create_dirs_durably(&fs, Path::new("/data")).unwrap();
    let meta = MetaStore::open(&fs, Path::new("/data")).unwrap();
    let name = TopicName::parse("t").unwrap();
    let parts = PartitionCount::new(2).unwrap();
    let topic = meta
        .create_topic(&name, parts, Default::default(), &Default::default())
        .unwrap();
    for p in parts.ids() {
        kafka_storage::topic_dirs::create_partition_dirs(&fs, Path::new("/data"), &name, parts)
            .unwrap();
        let _ = p;
    }
    (Partitions::new(fs, Config::new("/data".into())), topic)
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
