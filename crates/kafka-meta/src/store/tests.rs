use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use kafka_storage::fs::Fs;
use kafka_storage::sim::{Cut, DataCut, Failure, NameCut, SimFs};
use kafka_wire::ids::{PartitionCount, TopicName};
use redb::{Database, ReadableTable};

use super::*;
use crate::backend::FileBackend;

const DATA: &str = "/data";

fn disk() -> SimFs {
    let fs = SimFs::new();
    fs.create_dir(Path::new(DATA)).unwrap();
    fs.sync_dir(Path::new("/")).unwrap();
    fs
}

fn open(fs: &SimFs) -> MetaStore {
    MetaStore::open(fs, Path::new(DATA)).unwrap()
}

fn name(s: &str) -> TopicName {
    TopicName::parse(s).unwrap()
}

fn create(store: &MetaStore, n: &str) -> Result<std::sync::Arc<crate::Topic>, MetaError> {
    let parts = PartitionCount::new(3).unwrap();
    store.create_topic(&name(n), parts, BTreeMap::new(), &HashSet::new())
}

#[test]
fn what_was_acknowledged_is_there_after_the_power_goes_and_comes_back() {
    let fs = disk();
    let store = open(&fs);
    let config = BTreeMap::from([("retention.ms".to_owned(), "1000".to_owned())]);
    let kept = store
        .create_topic(
            &name("kept"),
            PartitionCount::new(5).unwrap(),
            config,
            &HashSet::new(),
        )
        .unwrap();
    create(&store, "gone").unwrap();
    store.delete_topic(&name("gone")).unwrap();
    drop(store);

    let cut = Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 1,
    };
    let store = open(&fs.power_cut(cut));
    assert_eq!(store.topics(), vec![kept.clone()]);
    assert_eq!(store.topic_by_id(kept.id), Some(kept));
    assert!(store.topic(&name("gone")).is_none());
}

#[test]
fn a_name_may_not_live_beside_one_that_differs_only_in_case_or_in_dot_for_underscore() {
    let store = open(&disk());
    create(&store, "a.b").unwrap();
    let refused = |n: &str, beside: &[&str]| {
        let beside = beside.iter().map(|b| name(b)).collect();
        let parts = PartitionCount::new(1).unwrap();
        match store.create_topic(&name(n), parts, BTreeMap::new(), &beside) {
            Err(MetaError::Collides(c)) => Some(c.case_only),
            Err(e) => panic!("{n}: {e}"),
            Ok(_) => None,
        }
    };
    assert!(matches!(
        create(&store, "a.b"),
        Err(MetaError::TopicExists(_))
    ));
    assert_eq!(refused("a_b", &[]), Some(false));
    assert_eq!(refused("A.B", &[]), Some(true));
    // A request's own earlier topics are let off the dot rule, and only that one.
    assert_eq!(refused("A.b", &["a.b"]), Some(true));
    assert_eq!(refused("a_b", &["a.b"]), None);
    // Deleting frees the name for the other spelling.
    store.delete_topic(&name("a.b")).unwrap();
    store.delete_topic(&name("a_b")).unwrap();
    assert_eq!(refused("A.B", &[]), None);
    assert_eq!(store.topics().len(), 1);
}

#[test]
fn producer_ids_never_repeat_across_restarts_and_cost_a_flush_once_a_block() {
    let fs = disk();
    let mut seen = Vec::new();
    for _ in 0..3 {
        let store = open(&fs);
        seen.push(store.next_producer_id().unwrap());
        let before = fs.ops();
        for _ in 0..500 {
            seen.push(store.next_producer_id().unwrap());
        }
        assert_eq!(
            fs.ops(),
            before,
            "ids inside a reserved block touch no file"
        );
    }
    assert!(seen.windows(2).all(|w| w[0] < w[1]), "{seen:?}");
}

#[test]
fn a_record_written_by_a_version_this_build_does_not_know_is_refused() {
    let fs = disk();
    let store = open(&fs);
    create(&store, "t").unwrap();
    drop(store);
    {
        let file = fs.open_existing(&Path::new(DATA).join(FILE)).unwrap();
        let db = Database::builder()
            .create_with_backend(FileBackend(file))
            .unwrap();
        let txn = db.begin_write().unwrap();
        {
            let mut topics = txn.open_table(TOPICS).unwrap();
            let mut record = topics.get("t").unwrap().unwrap().value().to_vec();
            record[0] = 99; // the rest is a good record
            topics.insert("t", record.as_slice()).unwrap();
        }
        txn.commit().unwrap();
    }
    assert!(matches!(
        MetaStore::open(&fs, Path::new(DATA)),
        Err(MetaError::Corrupt(_))
    ));
}

#[test]
fn a_write_that_fails_leaves_the_topics_as_they_were() {
    let setup = || {
        let fs = disk();
        let store = open(&fs);
        create(&store, "before").unwrap();
        (fs, store)
    };
    let (fs, store) = setup();
    let from = fs.ops();
    create(&store, "x").unwrap();
    let cost = fs.ops() - from;
    assert!(cost > 2, "a create is more than one file operation");
    for n in 1..=cost {
        let (fs, store) = setup();
        fs.fail_any(n, Failure::Io);
        let failed = create(&store, "x").is_err();
        assert_eq!(store.topic(&name("x")).is_some(), !failed, "operation {n}");
        assert!(store.topic(&name("before")).is_some());
        assert!(failed, "operation {n} of {cost} is part of the commit");
    }
}
