//! Things that go wrong only when requests really run at the same time: these use several threads.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::sync::Arc;

use kafka_storage::sim::{Cut, DataCut, NameCut};
use support::{records, rig, stored, topic};

/// A topic deleted by one client while another makes one of the same name: the new topic must be whole, and an
/// acknowledged write to it must survive.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_topic_made_again_while_its_old_self_is_being_deleted_is_not_damaged_by_the_delete() {
    for round in 0..12 {
        let rig = Arc::new(rig());
        rig.create(vec![topic("t", 150)], false).await;
        // Open every partition, so that deleting has writers to close (and so yields).
        for p in 0..150 {
            rig.produce(1, "t", p, records(1)).await;
        }
        let deleting = {
            let rig = Arc::clone(&rig);
            tokio::spawn(async move { rig.delete_by_name(&["t"]).await })
        };
        let making = {
            let rig = Arc::clone(&rig);
            tokio::spawn(async move {
                loop {
                    let made = rig.create(vec![topic("t", 150)], false).await.remove(0);
                    match made.error_code {
                        // Made: write to it at once, and be told it is durable, while the delete may still be going.
                        0 => return rig.produce(-1, "t", 1, records(1)).await,
                        36 => tokio::task::yield_now().await, // not yet deleted
                        other => panic!("create answered {other}: {:?}", made.error_message),
                    }
                }
            })
        };
        let (gone, acked) = tokio::join!(deleting, making);
        assert_eq!(gone.unwrap()[0].error_code, 0, "round {round}");
        let acked = acked.unwrap();
        assert_eq!(
            (acked.error_code, acked.base_offset),
            (0, 0),
            "round {round}: the new topic works"
        );
        let cut = rig.fs.power_cut(Cut {
            data: DataCut::Drop,
            names: NameCut::Durable,
            seed: round,
        });
        assert_eq!(
            stored(&cut, "t", 1).len(),
            1,
            "round {round}: what was acknowledged to the new topic survived"
        );
    }
}

/// The same race, with the new topic made by a Metadata request (auto-creation) instead of CreateTopics: that path
/// has its own lock to take.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_topic_made_by_a_metadata_request_while_its_old_self_is_being_deleted_is_not_damaged_by_the_delete()
 {
    for round in 0..12 {
        let rig = Arc::new(support::rig_with(|c| c.default_partitions = 150));
        rig.create(vec![topic("t", 150)], false).await;
        for p in 0..150 {
            rig.produce(1, "t", p, records(1)).await;
        }
        let old_id = rig.metadata(12, Some(&["t"]), false).await.topics[0].topic_id;
        let deleting = {
            let rig = Arc::clone(&rig);
            tokio::spawn(async move { rig.delete_by_name(&["t"]).await })
        };
        let making = {
            let rig = Arc::clone(&rig);
            tokio::spawn(async move {
                loop {
                    let answer = rig.metadata(12, Some(&["t"]), true).await;
                    let t = &answer.topics[0];
                    if t.error_code == 0 && t.topic_id != old_id {
                        return rig.produce(-1, "t", 1, records(1)).await;
                    }
                    tokio::task::yield_now().await;
                }
            })
        };
        let (gone, acked) = tokio::join!(deleting, making);
        assert_eq!(gone.unwrap()[0].error_code, 0, "round {round}");
        let acked = acked.unwrap();
        assert_eq!(
            (acked.error_code, acked.base_offset),
            (0, 0),
            "round {round}"
        );
        let cut = rig.fs.power_cut(Cut {
            data: DataCut::Drop,
            names: NameCut::Durable,
            seed: round,
        });
        assert_eq!(
            stored(&cut, "t", 1).len(),
            1,
            "round {round}: what was acknowledged survived"
        );
    }
}
