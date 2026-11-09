use std::path::Path;

use kafka_storage::fs::Fs;
use kafka_storage::sim::{Cut, DataCut, NameCut, Op, SimFs};
use kafka_wire::ids::GroupId;

use super::*;

fn store() -> (SimFs, MetaStore) {
    let fs = SimFs::new();
    fs.create_dir(Path::new("/data")).unwrap();
    fs.sync_dir(Path::new("/")).unwrap();
    let store = MetaStore::open(&fs, Path::new("/data")).unwrap();
    (fs, store)
}

fn group(g: &str) -> GroupId {
    GroupId::parse(g).unwrap()
}

fn life(live: bool, empty_since: Option<i64>) -> GroupLife {
    GroupLife {
        live,
        session_ms: 10_000,
        empty_since_ms: empty_since,
        protocol_type: "consumer".into(),
        consumer: None,
    }
}

#[test]
fn a_record_is_replaced_listed_by_id_and_forgotten_and_survives_the_power_going() {
    let (fs, store) = store();
    assert!(store.group_lives().unwrap().is_empty());
    store
        .set_group_life(&group("b"), &life(true, None))
        .unwrap();
    store
        .set_group_life(&group("a"), &life(false, Some(5)))
        .unwrap();
    store
        .set_group_life(&group("b"), &life(false, Some(9)))
        .unwrap();
    let all = store.group_lives().unwrap();
    assert_eq!(
        all.iter()
            .map(|(g, l)| (g.to_string(), l.empty_since_ms))
            .collect::<Vec<_>>(),
        [("a".into(), Some(5)), ("b".into(), Some(9))]
    );
    assert_eq!(
        store.group_life(&group("a")).unwrap(),
        Some(life(false, Some(5)))
    );
    assert_eq!(store.group_life(&group("zzz")).unwrap(), None);
    drop(store);
    let cut = Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 1,
    };
    let store = MetaStore::open(&fs.power_cut(cut), Path::new("/data")).unwrap();
    assert_eq!(store.group_lives().unwrap().len(), 2);
    store.forget_group_life(&group("a")).unwrap();
    assert_eq!(store.group_lives().unwrap().len(), 1);
}

#[test]
fn forgetting_what_is_not_there_writes_nothing() {
    let (fs, store) = store();
    let before = fs.count(Op::Fsync);
    store.forget_group_life(&group("nope")).unwrap();
    assert_eq!(fs.count(Op::Fsync), before);
}
