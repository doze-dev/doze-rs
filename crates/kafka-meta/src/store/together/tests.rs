//! Commits from different groups that arrive together share one flush, each is durable once answered, and a flush that
//! fails fails every one of them. On the simulated disk, which counts flushes.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::thread;

use kafka_storage::fs::Fs;
use kafka_storage::sim::{Cut, DataCut, Failure, NameCut, Op, SimFs};
use kafka_wire::ids::{GroupId, PartitionCount, PartitionId, TopicName};

use super::super::lock;
use crate::offsets::{Committed, OffsetCommit, StoredOffset};
use crate::{MetaError, MetaStore};

/// How many commits arrive together.
const N: usize = 8;

fn store() -> (SimFs, MetaStore) {
    let fs = SimFs::new();
    fs.create_dir(Path::new("/data")).unwrap();
    fs.sync_dir(Path::new("/")).unwrap();
    let store = MetaStore::open(&fs, Path::new("/data")).unwrap();
    let parts = PartitionCount::new(3).unwrap();
    store
        .create_topic(
            &TopicName::parse("t").unwrap(),
            parts,
            BTreeMap::new(),
            &HashSet::new(),
        )
        .unwrap();
    (fs, store)
}

fn group(i: usize) -> GroupId {
    GroupId::parse(&format!("g{i}")).unwrap()
}

fn at(offset: i64) -> OffsetCommit {
    OffsetCommit {
        topic: TopicName::parse("t").unwrap(),
        partition: PartitionId::new(1).unwrap(),
        offset: StoredOffset {
            offset,
            leader_epoch: -1,
            metadata: String::new(),
            committed_ms: 1000,
        },
    }
}

/// The flushes one commit makes on its own.
fn flushes_of_one(fs: &SimFs, store: &MetaStore) -> u64 {
    let before = fs.count(Op::Fsync);
    store.commit_offsets(&group(99), &[at(1)]).unwrap();
    fs.count(Op::Fsync) - before
}

/// `N` groups commit at once: the lock topic changes take is held until all `N` are queued, as a flush under way holds
/// the next ones, so they meet in one batch. Returns each group's answer.
fn together(store: &MetaStore) -> Vec<Result<Vec<Committed>, MetaError>> {
    thread::scope(|s| {
        let topics = lock(&store.write);
        let commits: Vec<_> = (0..N)
            .map(|i| s.spawn(move || store.commit_offsets(&group(i), &[at(10 + i as i64)])))
            .collect();
        while store.together.queued() < N {
            thread::yield_now();
        }
        drop(topics);
        commits.into_iter().map(|c| c.join().unwrap()).collect()
    })
}

#[test]
fn commits_from_different_groups_that_arrive_together_share_one_flush_and_each_survives_the_power_going()
 {
    let (fs, store) = store();
    let one = flushes_of_one(&fs, &store);
    assert!(one > 0);
    let before = fs.count(Op::Fsync);
    for answer in together(&store) {
        assert_eq!(answer.unwrap(), [Committed::Stored]);
    }
    let flushes = fs.count(Op::Fsync) - before;
    assert_eq!(flushes, one, "{N} commits, the flushes of one");
    drop(store);
    let cut = Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 7,
    };
    let store = MetaStore::open(&fs.power_cut(cut), Path::new("/data")).unwrap();
    let (t, p) = (TopicName::parse("t").unwrap(), PartitionId::new(1).unwrap());
    for i in 0..N {
        let found = store.offset(&group(i), &t, p).unwrap().map(|o| o.offset);
        assert_eq!(
            found,
            Some(10 + i as i64),
            "group {i}: answered, so durable"
        );
    }
}

#[test]
fn a_flush_that_fails_fails_every_commit_it_held() {
    let (fs, store) = store();
    fs.fail(Op::Fsync, 1, Failure::Io);
    let answers = together(&store);
    assert_eq!(answers.len(), N);
    for answer in answers {
        let e = answer.unwrap_err();
        assert!(
            matches!(e, MetaError::Database(_) | MetaError::Io(_)),
            "{e}"
        );
    }
}

#[test]
fn a_commit_alone_is_written_at_once_and_one_that_stores_nothing_is_not_flushed() {
    let (fs, store) = store();
    let one = flushes_of_one(&fs, &store);
    let before = fs.count(Op::Fsync);
    store.commit_offsets(&group(1), &[at(5)]).unwrap();
    assert_eq!(fs.count(Op::Fsync) - before, one);
    let gone = OffsetCommit {
        topic: TopicName::parse("gone").unwrap(),
        ..at(5)
    };
    let before = fs.count(Op::Fsync);
    let answer = store.commit_offsets(&group(1), &[gone]).unwrap();
    assert_eq!(answer, [Committed::UnknownTopicOrPartition]);
    assert_eq!(fs.count(Op::Fsync), before);
}
