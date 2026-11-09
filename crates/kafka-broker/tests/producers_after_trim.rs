//! A transactional producer whose data the log no longer holds, but whose commit marker it does: the broker answers the
//! producer's next sequence the same after the partition is opened again as before.
//!
//! Kafka keeps a producer's state when the log start moves (`ProducerStateManager.truncateAndReload`: "do not remove
//! producer state from the map") and, for a producer whose records were deleted by retention or DeleteRecords, accepts
//! any sequence (`ProducerAppendInfo.checkSequence`: "accept writes with any sequence number").
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use support::{compacted, delete_records, from, open_on, rig, txn_produce};

const OUT_OF_ORDER_SEQUENCE_NUMBER: i16 = 45;

#[tokio::test]
async fn a_transactional_producer_whose_data_was_deleted_but_whose_marker_was_not_continues_after_a_reopen_as_it_does_before()
 {
    for restart in [false, true] {
        let mut rig = rig();
        rig.create(vec![compacted("compact,delete")], false).await;
        let t = rig.init_txn("tx", 60_000).await.unwrap();
        assert_eq!(rig.add_partitions("tx", t, "c", &[0]).await, [0]);
        let batch = from(t.producer, t.epoch, 0, true, &[("k", "v")]);
        assert_eq!(txn_produce(&rig, "tx", batch).await, (0, 0));
        assert_eq!(rig.end_txn("tx", t, true).await, 0); // the marker is at offset 1
        assert_eq!(delete_records(&rig, 1).await, (0, 1)); // the data goes, the marker stays
        if restart {
            rig = open_on(rig.fs.process_restart());
        }
        assert_eq!(rig.add_partitions("tx", t, "c", &[0]).await, [0]);
        let next = from(t.producer, t.epoch, 1, true, &[("k", "w")]);
        let (e, base) = txn_produce(&rig, "tx", next).await;
        assert_eq!(
            (e, base),
            (0, 2),
            "restart={restart}: sequence 1 after the data is gone"
        );
        assert_eq!(rig.end_txn("tx", t, true).await, 0);
    }
}

#[tokio::test]
async fn a_new_epoch_raised_by_a_marker_still_starts_at_sequence_zero_after_a_reopen() {
    for restart in [false, true] {
        let mut rig = rig();
        rig.create(vec![compacted("compact,delete")], false).await;
        let t = rig.init_txn("tx", 60_000).await.unwrap();
        assert_eq!(rig.add_partitions("tx", t, "c", &[0]).await, [0]);
        let batch = from(t.producer, t.epoch, 0, true, &[("k", "v")]);
        assert_eq!(txn_produce(&rig, "tx", batch).await, (0, 0));
        // A new owner of the id fences the first and aborts what it left open, with markers at the raised epoch.
        let t2 = rig.init_txn("tx", 60_000).await.unwrap();
        assert_eq!(t2.producer, t.producer);
        assert!(t2.epoch > t.epoch);
        if restart {
            rig = open_on(rig.fs.process_restart());
        }
        assert_eq!(rig.add_partitions("tx", t2, "c", &[0]).await, [0]);
        let gap = from(t2.producer, t2.epoch, 1, true, &[("k", "w")]);
        assert_eq!(
            txn_produce(&rig, "tx", gap).await.0,
            OUT_OF_ORDER_SEQUENCE_NUMBER,
            "restart={restart}: a new epoch starts at 0"
        );
        let first = from(t2.producer, t2.epoch, 0, true, &[("k", "w")]);
        assert_eq!(
            txn_produce(&rig, "tx", first).await.0,
            0,
            "restart={restart}"
        );
    }
}
