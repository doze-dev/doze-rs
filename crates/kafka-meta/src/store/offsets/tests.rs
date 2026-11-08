use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use kafka_storage::fs::Fs;
use kafka_storage::sim::{Cut, DataCut, NameCut, Op, SimFs};
use kafka_wire::ids::{GroupId, PartitionCount, PartitionId, TopicName};

use super::*;

fn store() -> (SimFs, MetaStore) {
    let fs = SimFs::new();
    fs.create_dir(Path::new("/data")).unwrap();
    fs.sync_dir(Path::new("/")).unwrap();
    let store = MetaStore::open(&fs, Path::new("/data")).unwrap();
    for t in ["t", "u"] {
        let name = TopicName::parse(t).unwrap();
        let parts = PartitionCount::new(3).unwrap();
        store
            .create_topic(&name, parts, BTreeMap::new(), &HashSet::new())
            .unwrap();
    }
    (fs, store)
}

fn group(g: &str) -> GroupId {
    GroupId::parse(g).unwrap()
}

fn at(topic: &str, partition: i32, offset: i64) -> OffsetCommit {
    OffsetCommit {
        topic: TopicName::parse(topic).unwrap(),
        partition: PartitionId::new(partition).unwrap(),
        offset: StoredOffset {
            offset,
            leader_epoch: -1,
            metadata: format!("m{offset}"),
            committed_ms: 1000 + offset,
        },
    }
}

fn read(store: &MetaStore, g: &str, topic: &str, partition: i32) -> Option<i64> {
    let (t, p) = (
        TopicName::parse(topic).unwrap(),
        PartitionId::new(partition).unwrap(),
    );
    store.offset(&group(g), &t, p).unwrap().map(|o| o.offset)
}

#[test]
fn a_commit_is_read_back_whole_and_survives_the_power_going() {
    let (fs, store) = store();
    let done = store
        .commit_offsets(&group("g"), &[at("t", 1, 42)])
        .unwrap();
    assert_eq!(done, [Committed::Stored]);
    let found = store
        .offset(
            &group("g"),
            &TopicName::parse("t").unwrap(),
            PartitionId::new(1).unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(found, at("t", 1, 42).offset);
    drop(store);
    let cut = Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 1,
    };
    let store = MetaStore::open(&fs.power_cut(cut), Path::new("/data")).unwrap();
    assert_eq!(read(&store, "g", "t", 1), Some(42));
}

#[test]
fn a_commit_to_a_topic_or_partition_that_does_not_exist_is_told_so_and_the_rest_is_stored() {
    let (_fs, store) = store();
    let done = store
        .commit_offsets(
            &group("g"),
            &[
                at("t", 0, 1),
                at("nope", 0, 2),
                at("t", 3, 3), // the topic has partitions 0..=2
                at("u", 2, 4),
            ],
        )
        .unwrap();
    assert_eq!(
        done,
        [
            Committed::Stored,
            Committed::UnknownTopicOrPartition,
            Committed::UnknownTopicOrPartition,
            Committed::Stored
        ]
    );
    assert_eq!(read(&store, "g", "t", 0), Some(1));
    assert_eq!(read(&store, "g", "t", 3), None);
    assert_eq!(read(&store, "g", "nope", 0), None);
}

#[test]
fn a_commit_that_stores_nothing_writes_nothing() {
    let (fs, store) = store();
    let flushes = fs.count(Op::Fsync);
    let done = store
        .commit_offsets(&group("g"), &[at("nope", 0, 1)])
        .unwrap();
    assert_eq!(done, [Committed::UnknownTopicOrPartition]);
    assert_eq!(fs.count(Op::Fsync), flushes);
}

#[test]
fn a_groups_offsets_are_its_own_even_when_another_groups_id_begins_with_it() {
    let (_fs, store) = store();
    store.commit_offsets(&group("g"), &[at("t", 0, 1)]).unwrap();
    store
        .commit_offsets(&group("g2"), &[at("t", 0, 2)])
        .unwrap();
    store
        .commit_offsets(&group("g\u{1}x"), &[at("t", 0, 3)])
        .unwrap();
    let mine = store.group_offsets(&group("g")).unwrap();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].offset.offset, 1);
    assert_eq!(store.delete_group_offsets(&group("g")).unwrap(), 1);
    assert_eq!(read(&store, "g2", "t", 0), Some(2));
    assert_eq!(read(&store, "g\u{1}x", "t", 0), Some(3));
}

#[test]
fn a_groups_offsets_come_by_topic_and_then_partition() {
    let (_fs, store) = store();
    store
        .commit_offsets(
            &group("g"),
            &[at("u", 1, 5), at("t", 2, 4), at("u", 0, 3), at("t", 0, 1)],
        )
        .unwrap();
    let order: Vec<_> = store
        .group_offsets(&group("g"))
        .unwrap()
        .iter()
        .map(|o| (o.topic.to_string(), o.partition.get()))
        .collect();
    assert_eq!(
        order,
        [
            ("t".into(), 0),
            ("t".into(), 2),
            ("u".into(), 0),
            ("u".into(), 1)
        ]
    );
}

#[test]
fn groups_with_offsets_lists_each_once_in_order_and_forgetting_the_last_offset_removes_the_group() {
    let (_fs, store) = store();
    store
        .commit_offsets(&group("b"), &[at("t", 0, 1), at("t", 1, 1)])
        .unwrap();
    store.commit_offsets(&group("a"), &[at("u", 0, 1)]).unwrap();
    let listed: Vec<_> = store
        .groups_with_offsets()
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(listed, ["a", "b"]);
    let t = TopicName::parse("t").unwrap();
    let p = |n| PartitionId::new(n).unwrap();
    let gone = store
        .delete_offsets(&group("b"), &[(t.clone(), p(0)), (t.clone(), p(2))])
        .unwrap();
    assert_eq!(gone, 1, "only what was there is counted");
    assert_eq!(store.delete_offsets(&group("b"), &[(t, p(1))]).unwrap(), 1);
    assert_eq!(store.groups_with_offsets().unwrap().len(), 1);
}

#[test]
fn deleting_a_topic_takes_its_offsets_and_a_topic_made_again_starts_clean() {
    let (_fs, store) = store();
    store
        .commit_offsets(&group("g"), &[at("t", 0, 1), at("u", 0, 2)])
        .unwrap();
    store.delete_topic(&TopicName::parse("t").unwrap()).unwrap();
    assert_eq!(read(&store, "g", "t", 0), None);
    assert_eq!(read(&store, "g", "u", 0), Some(2), "another topic's stay");
    let name = TopicName::parse("t").unwrap();
    let parts = PartitionCount::new(1).unwrap();
    store
        .create_topic(&name, parts, BTreeMap::new(), &HashSet::new())
        .unwrap();
    assert_eq!(read(&store, "g", "t", 0), None);
}
