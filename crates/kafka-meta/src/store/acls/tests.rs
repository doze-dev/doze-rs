use std::collections::HashSet;
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

fn acl(name: &str, principal: &str) -> AclRecord {
    AclRecord {
        resource_type: 2,
        name: name.into(),
        pattern_type: 3,
        principal: principal.into(),
        host: "*".into(),
        operation: 8,
        permission: 3,
    }
}

fn sorted(mut v: Vec<AclRecord>) -> Vec<AclRecord> {
    v.sort();
    v
}

#[test]
fn a_new_store_has_none_and_what_is_added_is_listed_once() {
    let store = open(&disk());
    assert_eq!(store.acls().unwrap(), vec![]);
    let (a, b) = (acl("a", "User:x"), acl("b", "User:x"));
    store.add_acls(&[a.clone(), b.clone(), a.clone()]).unwrap();
    assert_eq!(sorted(store.acls().unwrap()), vec![a.clone(), b.clone()]);
    // The same ACL again is the same ACL.
    store.add_acls(std::slice::from_ref(&a)).unwrap();
    assert_eq!(store.acls().unwrap().len(), 2);
    store.add_acls(&[]).unwrap();
    store.remove_acls(&[a]).unwrap();
    assert_eq!(store.acls().unwrap(), vec![b]);
    // Removing what is not there is not an error.
    store.remove_acls(&[acl("never", "User:x")]).unwrap();
    assert_eq!(store.acls().unwrap().len(), 1);
}

#[test]
fn acls_survive_a_restart_and_a_power_cut_after_the_answer() {
    let fs = disk();
    let store = open(&fs);
    let kept = acl("kept", "User:alice");
    store
        .add_acls(&[kept.clone(), acl("gone", "User:alice")])
        .unwrap();
    store.remove_acls(&[acl("gone", "User:alice")]).unwrap();
    store.close().unwrap();
    assert_eq!(open(&fs).acls().unwrap(), vec![kept.clone()]);
    let cut = Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 3,
    };
    let store = open(&fs.power_cut(cut));
    assert_eq!(store.acls().unwrap(), vec![kept]);
}

#[test]
fn a_database_made_before_acls_existed_reads_as_none_and_takes_a_write() {
    let fs = disk();
    drop(open(&fs));
    {
        let file = fs.open_existing(&Path::new(DATA).join(FILE)).unwrap();
        let db = Database::builder()
            .create_with_backend(FileBackend(file))
            .unwrap();
        let txn = db.begin_write().unwrap();
        assert!(txn.delete_table(ACLS).unwrap());
        txn.commit().unwrap();
    }
    let store = open(&fs);
    assert_eq!(store.acls().unwrap(), vec![]);
    store.remove_acls(&[acl("a", "User:x")]).unwrap();
    store.add_acls(&[acl("a", "User:x")]).unwrap();
    assert_eq!(store.acls().unwrap().len(), 1);
}

#[test]
fn a_write_that_fails_leaves_the_acls_as_they_were() {
    let setup = || {
        let fs = disk();
        let store = open(&fs);
        store.add_acls(&[acl("before", "User:x")]).unwrap();
        (fs, store)
    };
    let (fs, store) = setup();
    let from = fs.ops();
    store
        .add_acls(&[acl("x", "User:x"), acl("y", "User:x")])
        .unwrap();
    let cost = fs.ops() - from;
    for n in 1..=cost {
        let (fs, store) = setup();
        fs.fail_any(n, Failure::Io);
        let failed = store
            .add_acls(&[acl("x", "User:x"), acl("y", "User:x")])
            .is_err();
        let now = store.acls().unwrap().len();
        // Both or neither: a request's ACLs are one transaction.
        assert!(now == 1 || now == 3, "operation {n}: {now} ACLs");
        assert_eq!(now == 3, !failed, "operation {n}");
    }
}

#[test]
fn writers_at_once_lose_nothing() {
    let fs = disk();
    let store = Arc::new(open(&fs));
    let threads: Vec<_> = (0..4)
        .map(|t| {
            let store = Arc::clone(&store);
            std::thread::spawn(move || {
                for i in 0..10 {
                    store
                        .add_acls(&[acl(&format!("t{t}-{i}"), "User:x")])
                        .unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let all: HashSet<_> = store.acls().unwrap().into_iter().collect();
    assert_eq!(all.len(), 40);
}
