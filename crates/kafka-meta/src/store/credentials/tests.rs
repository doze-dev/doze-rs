use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use kafka_storage::fs::Fs;
use kafka_storage::sim::{Cut, DataCut, Failure, NameCut, SimFs};
use redb::Database;

use super::*;
use crate::store::FILE;
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

fn id(user: &str, mechanism: i8) -> CredentialId {
    CredentialId {
        user: user.into(),
        mechanism,
    }
}

fn record(tag: u8) -> CredentialRecord {
    CredentialRecord {
        salt: vec![tag; 8],
        iterations: 4096 + u32::from(tag),
        stored_key: vec![tag; 32],
        server_key: vec![tag.wrapping_add(1); 32],
    }
}

fn all(store: &MetaStore) -> HashMap<CredentialId, CredentialRecord> {
    store.credentials().unwrap().into_iter().collect()
}

#[test]
fn a_new_store_has_none_and_a_put_replaces_the_same_user_and_mechanism() {
    let store = open(&disk());
    assert!(store.credentials().unwrap().is_empty());
    store
        .put_credentials(&[(id("alice", 1), record(1)), (id("alice", 2), record(2))])
        .unwrap();
    assert_eq!(all(&store).len(), 2);
    store
        .put_credentials(&[(id("alice", 1), record(9))])
        .unwrap();
    let now = all(&store);
    assert_eq!(now.len(), 2);
    assert_eq!(now[&id("alice", 1)], record(9));
    assert_eq!(now[&id("alice", 2)], record(2));
    store.put_credentials(&[]).unwrap();
    assert_eq!(all(&store).len(), 2);
}

#[test]
fn a_removal_removes_only_what_it_names_and_what_is_not_there_is_not_an_error() {
    let store = open(&disk());
    store
        .put_credentials(&[(id("alice", 1), record(1)), (id("alice", 2), record(2))])
        .unwrap();
    store.remove_credentials(&[id("alice", 1)]).unwrap();
    assert_eq!(all(&store), HashMap::from([(id("alice", 2), record(2))]));
    store
        .remove_credentials(&[id("nobody", 1), id("alice", 1)])
        .unwrap();
    assert_eq!(all(&store).len(), 1);
    store.remove_credentials(&[]).unwrap();
}

#[test]
fn one_alteration_removes_and_stores_together() {
    let store = open(&disk());
    store.put_credentials(&[(id("old", 1), record(1))]).unwrap();
    store
        .alter_credentials(&[(id("new", 2), record(2))], &[id("old", 1)])
        .unwrap();
    assert_eq!(all(&store), HashMap::from([(id("new", 2), record(2))]));
}

#[test]
fn credentials_survive_a_restart_and_a_power_cut_after_the_answer() {
    let fs = disk();
    let store = open(&fs);
    store
        .put_credentials(&[(id("kept", 2), record(5)), (id("gone", 1), record(6))])
        .unwrap();
    store.remove_credentials(&[id("gone", 1)]).unwrap();
    store.close().unwrap();
    let kept = HashMap::from([(id("kept", 2), record(5))]);
    assert_eq!(all(&open(&fs)), kept);
    let cut = Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 3,
    };
    assert_eq!(all(&open(&fs.power_cut(cut))), kept);
}

#[test]
fn a_database_made_before_credentials_existed_reads_as_none_and_takes_a_write() {
    let fs = disk();
    drop(open(&fs));
    {
        let file = fs.open_existing(&Path::new(DATA).join(FILE)).unwrap();
        let db = Database::builder()
            .create_with_backend(FileBackend(file))
            .unwrap();
        let txn = db.begin_write().unwrap();
        assert!(txn.delete_table(CREDENTIALS).unwrap());
        txn.commit().unwrap();
    }
    let store = open(&fs);
    assert!(store.credentials().unwrap().is_empty());
    store.remove_credentials(&[id("a", 1)]).unwrap();
    store.put_credentials(&[(id("a", 1), record(1))]).unwrap();
    assert_eq!(all(&store).len(), 1);
}

#[test]
fn a_write_that_fails_leaves_the_credentials_as_they_were() {
    let setup = || {
        let fs = disk();
        let store = open(&fs);
        store
            .put_credentials(&[(id("before", 1), record(1))])
            .unwrap();
        (fs, store)
    };
    let change = |store: &MetaStore| {
        store.alter_credentials(
            &[(id("x", 1), record(2)), (id("y", 2), record(3))],
            &[id("before", 1)],
        )
    };
    let (fs, store) = setup();
    let from = fs.ops();
    change(&store).unwrap();
    let cost = fs.ops() - from;
    for n in 1..=cost {
        let (fs, store) = setup();
        fs.fail_any(n, Failure::Io);
        let failed = change(&store).is_err();
        let now = all(&store);
        // The removal and both puts are one transaction: all of it or none of it.
        assert!(
            now.len() == 1 && now.contains_key(&id("before", 1))
                || now.len() == 2 && !now.contains_key(&id("before", 1)),
            "operation {n}: {now:?}"
        );
        assert_eq!(now.len() == 2, !failed, "operation {n}");
    }
}

#[test]
fn writers_at_once_lose_nothing() {
    let store = Arc::new(open(&disk()));
    let threads: Vec<_> = (0..4)
        .map(|t| {
            let store = Arc::clone(&store);
            std::thread::spawn(move || {
                for i in 0..10 {
                    store
                        .put_credentials(&[(id(&format!("t{t}-{i}"), 1), record(1))])
                        .unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(all(&store).len(), 40);
}
