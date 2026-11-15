use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use kafka_storage::fs::Fs;
use kafka_storage::sim::{Cut, DataCut, NameCut, Op, SimFs};
use kafka_wire::ids::{GroupId, PartitionCount, PartitionId, TopicName};

use super::*;
use crate::share::{ShareBatch, ShareRecordState};

fn store() -> (SimFs, MetaStore) {
    let fs = SimFs::new();
    fs.create_dir(Path::new("/data")).unwrap();
    fs.sync_dir(Path::new("/")).unwrap();
    let store = MetaStore::open(&fs, Path::new("/data")).unwrap();
    for t in ["t", "u"] {
        let name = TopicName::parse(t).unwrap();
        let parts = PartitionCount::new(2).unwrap();
        store
            .create_topic(&name, parts, BTreeMap::new(), &HashSet::new())
            .unwrap();
    }
    (fs, store)
}

fn group(g: &str) -> GroupId {
    GroupId::parse(g).unwrap()
}

fn topic(t: &str) -> TopicName {
    TopicName::parse(t).unwrap()
}

fn part(p: i32) -> PartitionId {
    PartitionId::new(p).unwrap()
}

fn state(start: i64) -> ShareState {
    ShareState {
        state_epoch: 1,
        start_offset: start,
        batches: vec![ShareBatch {
            first: start + 1,
            last: start + 2,
            state: ShareRecordState::Acknowledged,
            delivery_count: 1,
        }],
    }
}

#[test]
fn states_are_written_together_and_survive_the_power_going() {
    let (fs, store) = store();
    let (t, u) = (topic("t"), topic("u"));
    let (a, b) = (state(5), state(9));
    let done = store
        .set_share_states(&group("g"), &[(&t, part(0), &a), (&u, part(1), &b)])
        .unwrap();
    assert_eq!(done, [true, true]);
    drop(store);
    let cut = Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 7,
    };
    let store = MetaStore::open(&fs.power_cut(cut), Path::new("/data")).unwrap();
    assert_eq!(
        store.share_state(&group("g"), &t, part(0)).unwrap(),
        Some(a)
    );
    let all = store.share_states(&group("g")).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!((all[1].0.as_str(), all[1].1.get(), &all[1].2), ("u", 1, &b));
}

#[test]
fn a_state_for_a_partition_that_is_not_there_is_not_stored_and_nothing_is_written_for_none() {
    let (fs, store) = store();
    let (t, nope) = (topic("t"), topic("nope"));
    let s = state(0);
    let flushes = fs.count(Op::Fsync);
    let done = store
        .set_share_states(&group("g"), &[(&nope, part(0), &s), (&t, part(2), &s)])
        .unwrap();
    assert_eq!(done, [false, false]);
    assert_eq!(
        fs.count(Op::Fsync),
        flushes,
        "nothing to store, nothing written"
    );
    assert!(store.share_states(&group("g")).unwrap().is_empty());
}

#[test]
fn a_groups_states_are_its_own_and_go_by_topic_or_all_at_once() {
    let (_fs, store) = store();
    let (t, u) = (topic("t"), topic("u"));
    let s = state(3);
    for g in ["g", "g2", "g\u{1}x"] {
        store
            .set_share_states(&group(g), &[(&t, part(0), &s), (&u, part(0), &s)])
            .unwrap();
    }
    assert_eq!(
        store
            .delete_share_states(&group("g"), Some(std::slice::from_ref(&u)))
            .unwrap(),
        1
    );
    let left: Vec<String> = store
        .share_states(&group("g"))
        .unwrap()
        .into_iter()
        .map(|(t, _, _)| t.as_str().to_owned())
        .collect();
    assert_eq!(left, ["t"]);
    assert_eq!(store.delete_share_states(&group("g"), None).unwrap(), 1);
    assert!(store.share_states(&group("g")).unwrap().is_empty());
    assert_eq!(store.share_states(&group("g2")).unwrap().len(), 2);
    assert_eq!(store.share_states(&group("g\u{1}x")).unwrap().len(), 2);
}

#[test]
fn a_deleted_topic_takes_its_share_states_with_it() {
    let (_fs, store) = store();
    let (t, u) = (topic("t"), topic("u"));
    let s = state(0);
    store
        .set_share_states(&group("g"), &[(&t, part(1), &s), (&u, part(1), &s)])
        .unwrap();
    store.delete_topic(&t).unwrap();
    let left = store.share_states(&group("g")).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].0.as_str(), "u");
}
