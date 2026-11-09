//! The files open partitions hold between them are bounded, not only their number: a partition with many segments costs
//! more than one with few.

use kafka_wire::batch::{Batch, TimestampType};
use kafka_wire::ids::TimestampMs;

use super::*;

async fn write(p: &Partition<SimFs>, records: u32) {
    let batch = Batch::parse(BatchBuilder::new().records(records).build()).unwrap();
    let pending = p
        .submit(
            batch,
            TimestampMs::from_wire(1),
            TimestampType::Create,
            true,
        )
        .await
        .unwrap();
    pending.await.unwrap().unwrap();
}

#[tokio::test]
async fn partitions_are_closed_to_keep_the_files_they_hold_within_the_budget() {
    let (partitions, topic) = setup_with(
        |c| {
            c.defaults.segment_bytes = 1_000;
            c.max_open_descriptors = 14;
        },
        3,
    );
    let id = |n| PartitionId::new(n).unwrap();
    // Partition 0 grows five or six segments: two files for each, and two more.
    let busy = partitions.get(&topic, id(0)).await.unwrap();
    for _ in 0..60 {
        write(&busy, 10).await;
    }
    let held = busy.descriptors();
    assert!(held >= 10, "{held} files");
    let end = busy.reader().end_offset().get();
    drop(busy);
    // A second, with one segment (four files), does not fit beside it: the idle one is closed to make room.
    drop(partitions.get(&topic, id(1)).await.unwrap());
    assert_eq!(partitions.len(), 1, "partition 0 was closed");
    // And it opens again, whole.
    let again = partitions.get(&topic, id(0)).await.unwrap();
    assert_eq!(again.reader().end_offset().get(), end);
    // One in use is never the one to go, even over the budget.
    let _held = partitions.get(&topic, id(1)).await.unwrap();
    assert!(partitions.get(&topic, id(2)).await.is_ok());
}
