//! Erase everything, and the usage the Data page reads (docs/port/data-control.md).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::path::Path;

use kafka_storage::fs::{Fs, FsFile};
use kafka_storage::usage::partition_of;
use support::acls::{grant, on};
use support::{caller, join_request, records, rig, topic, with_config};

const READ: i8 = 3;

#[tokio::test]
async fn erase_everything_leaves_an_empty_broker_with_its_acls_and_the_groups_still_connected() {
    let rig = rig();
    rig.create(
        vec![
            topic("a", 2),
            with_config(topic("b", 1), "cleanup.policy", "compact"),
        ],
        false,
    )
    .await;
    rig.produce(-1, "a", 0, records(10)).await;
    rig.produce(-1, "b", 0, support::keyed(&[("k", Some("v"))]))
        .await;
    rig.commit_offsets(2, "done", "", -1, &[("a", 0, 7, None)])
        .await;
    let txn = rig.init_txn("finished", 60_000).await.unwrap();
    rig.add_partitions("finished", txn, "a", &[1]).await;
    rig.produce_txn("finished", txn, ("a", 1), 0, 1).await;
    assert_eq!(rig.end_txn("finished", txn, true).await, 0);
    rig.init_txn("idle", 60_000).await.unwrap();
    let joined = rig
        .join(3, join_request("live", "", None, &[("range", "")]))
        .await;
    assert_eq!(joined.error_code, 0);
    grant(&rig, vec![on("a", "User:bob", READ)]).await;
    // A directory left by a metadata store that was lost: no topic owns it.
    rig.fs.create_dir(Path::new("/data/ghost-0")).unwrap();
    let (f, _) = rig
        .fs
        .open_or_create(Path::new("/data/ghost-0/00000000000000000000.log"))
        .unwrap();
    f.write_all_at(&[1; 64], 0).unwrap();
    let before = rig.broker.data_usage(&caller()).unwrap();
    assert_eq!(before.strays.len(), 1);
    assert_eq!(before.groups, 1);
    assert_eq!(before.transactional_ids, 2);

    let erased = rig.broker.erase_everything(&caller()).await.unwrap();
    assert_eq!(erased.topics, 2);
    assert_eq!(erased.groups, 1, "the group that only had commits");
    assert_eq!(erased.groups_kept, vec!["live".to_owned()]);
    assert_eq!(erased.transactional_ids, 2);
    assert_eq!(
        erased.freed_bytes,
        before.topics_bytes() + before.stray_bytes()
    );

    assert!(rig.topic_names().await.is_empty());
    let left: Vec<String> = rig
        .dirs()
        .into_iter()
        .filter(|d| partition_of(d).is_some())
        .collect();
    assert!(left.is_empty(), "partition directories left: {left:?}");
    assert!(rig.fetch_offsets(false, "done", None).await.is_empty());
    assert_eq!(rig.broker.acl_count(), 1, "the ACLs stay");
    let after = rig.broker.data_usage(&caller()).unwrap();
    assert!(after.topics.is_empty() && after.strays.is_empty());
    assert_eq!((after.groups, after.transactional_ids), (0, 0));
    // A topic made again with an old name is a new one: it starts at 0. (Not `a`: the ACL kept on it is bob's alone.)
    rig.create(vec![topic("b", 1)], false).await;
    assert_eq!(rig.produce(-1, "b", 0, records(1)).await.base_offset, 0);
}

#[tokio::test]
async fn erase_is_refused_while_a_transaction_is_under_way() {
    let rig = rig();
    rig.create(vec![topic("a", 1)], false).await;
    let txn = rig.init_txn("busy", 60_000).await.unwrap();
    rig.add_partitions("busy", txn, "a", &[0]).await;
    let refused = rig.broker.erase_everything(&caller()).await.unwrap_err();
    assert_eq!(
        refused,
        kafka_broker::DataRefusal::TxnOpen {
            topic: None,
            id: Some("busy".into())
        }
    );
    assert_eq!(
        rig.topic_names().await,
        vec!["a".to_owned()],
        "nothing was touched"
    );
}

#[tokio::test]
async fn the_usage_says_each_kind_each_topic_largest_first_and_what_it_keeps() {
    let rig = rig();
    rig.fs.set_capacity(Some(1 << 30));
    rig.create(
        vec![
            topic("small", 1),
            with_config(topic("big", 2), "retention.ms", "604800000"),
            with_config(topic("keys", 1), "cleanup.policy", "compact"),
        ],
        false,
    )
    .await;
    rig.produce(-1, "small", 0, records(1)).await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    for p in [0, 1] {
        let fresh = kafka_wire::batch::build::BatchBuilder::new()
            .records(50)
            .timestamps(now, now)
            .build();
        rig.produce(-1, "big", p, fresh).await;
    }
    let u = rig.broker.data_usage(&caller()).unwrap();
    let names: Vec<&str> = u.topics.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        ["big", "small", "keys"],
        "largest first, then by name"
    );
    let big = &u.topics[0];
    assert_eq!((big.partitions, big.segments), (2, 2));
    assert_eq!(big.keeps.max_age_ms, Some(604_800_000));
    assert!(big.keeps.own && !big.keeps.unbounded());
    assert!(big.oldest_ms.is_some() && big.last_write_ms.is_some());
    assert!(
        big.keeps
            .why(Some(3 * 86_400_000))
            .contains("Retention 7 days")
    );
    let small = &u.topics[1];
    assert!(
        small.keeps.unbounded(),
        "no retention, deletes: it keeps everything"
    );
    assert!(u.defaults.unbounded(), "the broker's default is no limit");
    assert!(u.meta_bytes > 0);
    assert_eq!(u.total(), u.topics_bytes() + u.meta_bytes + u.other_bytes);
    let space = u.space.unwrap();
    assert_eq!(space.total, 1 << 30);
    assert!(space.free < space.total);
    // A pass of retention works out when the oldest segment can go; the usage says it.
    rig.broker.check_logs().await;
    let u = rig.broker.data_usage(&caller()).unwrap();
    let due = u
        .topics
        .iter()
        .find(|t| t.name == "big")
        .unwrap()
        .next_drop_ms;
    assert!(due.is_some(), "the last pass said when the oldest can go");
}
