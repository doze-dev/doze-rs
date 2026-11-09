//! A compacted topic through the broker: its cleaner leaves the latest record of each key at the offset it had, and a
//! consumer reading it sees that; retention and compaction each act on the policies that ask for them.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_wire::batch::Batches;
use support::{keyed, open_on, rig, topic, with_config};

/// A record as a consumer sees it: offset, key, whether it is a deletion.
type Got = (i64, String, bool);

/// A compacted topic with small segments, cleaned whenever anything is dirty.
fn compacted(policy: &str) -> kafka_wire::messages::create_topics_request::CreatableTopic {
    let t = with_config(topic("c", 1), "cleanup.policy", policy);
    let t = with_config(t, "segment.bytes", "1024");
    let t = with_config(t, "min.cleanable.dirty.ratio", "0.01");
    with_config(t, "delete.retention.ms", "1")
}

/// Every record the topic serves from the start, read the way a consumer does.
async fn everything(rig: &support::Rig, topic: &str) -> Vec<Got> {
    let (mut at, end) = (0, rig.list_offset(7, topic, 0, -1).await.offset);
    let mut out = Vec::new();
    while at < end {
        let data = rig.fetch_now(&[(topic, 0, at, 1 << 20)]).await.remove(0);
        assert_eq!(data.error_code, 0);
        let bytes = data.records.expect("records");
        assert!(!bytes.is_empty(), "nothing at {at} before the end {end}");
        for b in Batches::new(bytes) {
            let b = b.unwrap();
            b.each_record(1 << 20, |s| {
                if s.offset >= at {
                    out.push((
                        s.offset,
                        String::from_utf8(s.key.unwrap().to_vec()).unwrap(),
                        s.tombstone,
                    ));
                }
            })
            .unwrap();
            at = b.next_offset().unwrap().get();
        }
    }
    out
}

/// `n` appends of one record each over `keys` keys (the value says which append), the last append of each key a deletion
/// if `delete_every` divides its number.
async fn fill(rig: &support::Rig, n: usize, keys: usize) {
    for i in 0..n {
        let value = format!("v{i}");
        let one = keyed(&[(&format!("k{}", i % keys), Some(&value))]);
        assert_eq!(rig.produce(-1, "c", 0, one).await.error_code, 0);
    }
}

#[tokio::test]
async fn a_compacted_topic_keeps_the_latest_record_of_each_key_at_its_own_offset() {
    let rig = rig();
    rig.create(vec![compacted("compact")], false).await;
    fill(&rig, 60, 5).await;
    let before = everything(&rig, "c").await;
    assert_eq!(before.len(), 60);
    rig.broker.check_cleaner().await;
    let after = everything(&rig, "c").await;
    assert_eq!(
        rig.list_offset(7, "c", 0, -1).await.offset,
        60,
        "the end does not move"
    );
    assert!(after.len() < before.len(), "{} records left", after.len());
    for k in 0..5 {
        let key = format!("k{k}");
        let latest = before.iter().rfind(|r| r.1 == key).unwrap();
        assert!(after.contains(latest), "the latest of {key} is gone");
    }
    assert!(
        after.iter().all(|r| before.contains(r)),
        "nothing that was not written"
    );
    // The topic is served from anywhere in it.
    // A fetch is never answered with nothing but a batch that holds no records while records follow: librdkafka, given
    // enough of those, gives up on the partition.
    for from in 0..60 {
        let data = rig.fetch_now(&[("c", 0, from, 1 << 20)]).await.remove(0);
        assert_eq!(data.error_code, 0, "from {from}");
        let first = Batches::new(data.records.expect("records"))
            .next()
            .unwrap_or_else(|| panic!("nothing from {from}"))
            .unwrap();
        assert!(
            first.record_count() > 0,
            "from {from}: a batch with no records first"
        );
    }
}

#[tokio::test]
async fn a_topic_that_does_not_compact_is_left_as_it_is_and_one_that_only_compacts_is_not_trimmed()
{
    let rig = rig();
    rig.create(vec![compacted("delete")], false).await;
    fill(&rig, 40, 3).await;
    rig.broker.check_cleaner().await;
    assert_eq!(
        everything(&rig, "c").await.len(),
        40,
        "a delete topic is not compacted"
    );
    let rig = support::rig();
    let t = with_config(compacted("compact"), "retention.bytes", "100");
    rig.create(vec![t], false).await;
    fill(&rig, 40, 3).await;
    rig.broker.check_logs().await;
    assert_eq!(
        rig.list_offset(7, "c", 0, -2).await.offset,
        0,
        "retention does not touch a compacted topic"
    );
    // With both policies retention applies as well, after compaction.
    let rig = support::rig();
    let t = with_config(compacted("compact,delete"), "retention.bytes", "300");
    rig.create(vec![t], false).await;
    fill(&rig, 60, 40).await;
    rig.broker.check_cleaner().await;
    assert!(
        everything(&rig, "c").await.len() < 60,
        "compact,delete compacts"
    );
    rig.broker.check_logs().await;
    assert!(
        rig.list_offset(7, "c", 0, -2).await.offset > 0,
        "retention applied"
    );
}

#[tokio::test]
async fn a_deletion_is_served_until_its_time_and_then_it_and_the_record_it_deleted_are_gone() {
    let rig = rig();
    rig.create(vec![compacted("compact")], false).await;
    fill(&rig, 30, 5).await;
    assert_eq!(
        rig.produce(-1, "c", 0, keyed(&[("k0", None)]))
            .await
            .error_code,
        0
    );
    fill(&rig, 30, 5).await;
    // k0 is written again after its deletion; delete a key that is not: a new one, written and deleted.
    assert_eq!(
        rig.produce(-1, "c", 0, keyed(&[("gone", Some("1"))]))
            .await
            .error_code,
        0
    );
    assert_eq!(
        rig.produce(-1, "c", 0, keyed(&[("gone", None)]))
            .await
            .error_code,
        0
    );
    fill(&rig, 30, 5).await;
    rig.broker.check_cleaner().await;
    let first = everything(&rig, "c").await;
    assert!(
        first.iter().any(|r| r.1 == "gone" && r.2),
        "the deletion is served for now: {first:?}"
    );
    assert!(
        !first.iter().any(|r| r.1 == "gone" && !r.2),
        "the value it deleted is not"
    );
    // Its time (a millisecond) has passed; more arrives, and the next round takes it.
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    fill(&rig, 30, 5).await;
    rig.broker.check_cleaner().await;
    assert!(
        !everything(&rig, "c").await.iter().any(|r| r.1 == "gone"),
        "gone with its deletion"
    );
}

#[tokio::test]
async fn what_cleaning_left_is_what_a_restarted_broker_serves() {
    let rig = rig();
    rig.create(vec![compacted("compact")], false).await;
    fill(&rig, 60, 5).await;
    rig.broker.check_cleaner().await;
    let served = everything(&rig, "c").await;
    let again = open_on(rig.fs.process_restart());
    assert_eq!(everything(&again, "c").await, served);
    assert_eq!(again.list_offset(7, "c", 0, -1).await.offset, 60);
    // And it goes on taking writes and cleaning.
    for i in 0..30 {
        let one = keyed(&[(&format!("k{}", i % 5), Some("new"))]);
        assert_eq!(again.produce(-1, "c", 0, one).await.error_code, 0);
    }
    again.broker.check_cleaner().await;
    assert_eq!(again.list_offset(7, "c", 0, -1).await.offset, 90);
}
