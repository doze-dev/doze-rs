use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use kafka_storage::fs::Fs;
use kafka_storage::sim::{Cut, DataCut, Failure, NameCut, SimFs};
use kafka_wire::ids::{PartitionCount, TopicName};
use redb::{Database, ReadableTable};

use super::*;
use crate::Topic;
use doze_store::FileBackend;

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

#[test]
fn the_topics_version_changes_when_a_topic_is_made_or_removed_and_not_otherwise() {
    let fs = disk();
    let store = open(&fs);
    let empty = store.topics_version();
    assert_eq!(store.topics_version(), empty, "reading changes nothing");
    create(&store, "t").unwrap();
    let one = store.topics_version();
    assert_ne!(one, empty);
    let _ = store.topics();
    let _ = store.topic(&name("t"));
    assert_eq!(store.topics_version(), one);
    store.delete_topic(&name("t")).unwrap();
    assert_ne!(store.topics_version(), one);
}

#[test]
fn a_changed_topic_keeps_its_name_and_id_is_durable_and_a_failed_write_changes_nothing() {
    let fs = disk();
    let store = open(&fs);
    let made = create(&store, "t").unwrap();
    let before = store.topics_version();
    let changed = store
        .update_topic(&name("t"), |_| Topic {
            name: name("other"),
            id: crate::random_topic_id().unwrap(),
            partitions: PartitionCount::new(9).unwrap(),
            config: BTreeMap::from([("retention.ms".to_owned(), "5".to_owned())]),
        })
        .unwrap()
        .unwrap();
    assert_eq!(
        (changed.name.clone(), changed.id),
        (made.name.clone(), made.id)
    );
    assert_eq!(changed.partitions.get(), 9);
    assert_ne!(store.topics_version(), before);
    assert_eq!(store.topic_by_id(made.id), Some(changed.clone()));
    assert!(
        store
            .update_topic(&name("nope"), Topic::clone)
            .unwrap()
            .is_none()
    );
    // Durable, and a write that fails leaves the topic as it was.
    drop(store);
    let cut = Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 2,
    };
    let disk = fs.power_cut(cut);
    let store = open(&disk);
    assert_eq!(store.topic(&name("t")), Some(changed.clone()));
    disk.fail_any(1, Failure::Io);
    assert!(
        store
            .update_topic(&name("t"), |t| Topic {
                partitions: PartitionCount::new(20).unwrap(),
                ..t.clone()
            })
            .is_err()
    );
    assert_eq!(store.topic(&name("t")), Some(changed));
}

#[test]
fn broker_configs_are_kept_by_scope_replaced_whole_and_survive_the_power_going() {
    let fs = disk();
    let store = open(&fs);
    assert!(store.broker_configs().unwrap().is_empty());
    let map = |pairs: &[(&str, &str)]| {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect::<BTreeMap<_, _>>()
    };
    store
        .set_broker_configs("", &map(&[("a", "1"), ("b", "2")]))
        .unwrap();
    store.set_broker_configs("1", &map(&[("a", "9")])).unwrap();
    // Setting a scope again leaves nothing of what it held, and does not touch the other scope.
    store.set_broker_configs("", &map(&[("c", "3")])).unwrap();
    let want = BTreeMap::from([
        (("".to_owned(), "c".to_owned()), "3".to_owned()),
        (("1".to_owned(), "a".to_owned()), "9".to_owned()),
    ]);
    assert_eq!(store.broker_configs().unwrap(), want);
    drop(store);
    let cut = Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 3,
    };
    let disk = fs.power_cut(cut);
    let store = open(&disk);
    assert_eq!(store.broker_configs().unwrap(), want);
    // A write that fails changes nothing.
    disk.fail_any(1, Failure::Io);
    assert!(store.set_broker_configs("", &map(&[("z", "0")])).is_err());
    assert_eq!(store.broker_configs().unwrap(), want);
}
