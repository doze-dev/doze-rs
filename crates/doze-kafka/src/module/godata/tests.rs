//! Against a folder the Go doze-kafka broker wrote (tests/fixtures/go-data: three topics of three partitions, records
//! produced and two groups' offsets committed with kcat, then a clean stop; `meta.db` cut at its high page).

use super::{TookOver, pending, take_over};
use kafka_meta::MetaStore;
use kafka_storage::fs::RealFs;
use kafka_wire::ids::{GroupId, PartitionId, TopicName};
use std::path::{Path, PathBuf};

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        if e.path().is_dir() {
            copy(&e.path(), &to.join(e.file_name()));
        } else {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

fn go_folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("doze-kafka-godata-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/go-data"),
        &dir,
    );
    dir
}

#[test]
fn go_metadata_becomes_the_brokers_once() {
    let dir = go_folder("once");
    assert!(pending(&dir));
    let done = take_over(&dir).unwrap().unwrap();
    assert_eq!((done.topics, done.offsets), (3, 3));
    assert_eq!(
        done.left,
        ["member-less groups' liveness (their offsets are kept)"]
    );
    assert!(!pending(&dir));
    assert!(dir.join("meta.redb").is_file() && dir.join("meta.db.go").is_file());
    assert!(
        !dir.join("meta.db").exists()
            && !dir.join("clean-shutdown").exists()
            && !dir.join("unvalidated").exists()
    );
    assert!(!dir.join(".meta-from-go").exists());
    assert_eq!(
        take_over(&dir).unwrap(),
        None::<TookOver>,
        "a second start takes nothing over"
    );

    let meta = MetaStore::open(&RealFs, &dir).unwrap();
    let orders = meta
        .topic(&TopicName::parse("orders.placed").unwrap())
        .unwrap();
    assert_eq!(orders.partitions.get(), 3);
    assert_eq!(
        orders.id.as_bytes(),
        &[
            192, 2, 16, 239, 157, 212, 163, 223, 91, 170, 172, 186, 126, 232, 218, 183
        ],
        "the topic keeps the id Go gave it"
    );
    let at = |g: &str, t: &str, p: i32| {
        meta.offset(
            &GroupId::parse(g).unwrap(),
            &TopicName::parse(t).unwrap(),
            PartitionId::new(p).unwrap(),
        )
        .unwrap()
        .map(|o| (o.offset, o.committed_ms))
    };
    assert_eq!(
        at("shippers", "orders.placed", 1),
        Some((3, 1_795_202_455_225))
    );
    assert_eq!(
        at("shippers", "orders.placed", 2),
        Some((1, 1_795_202_455_225))
    );
    assert_eq!(at("billing", "payments", 0), Some((2, 1_795_202_456_124)));
    assert_eq!(at("billing", "payments", 1), None);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_damaged_go_database_stops_the_start_and_is_left_as_it_was() {
    let dir = go_folder("damaged");
    std::fs::write(dir.join("meta.db"), b"not bbolt").unwrap();
    assert!(take_over(&dir).unwrap_err().contains("meta.db"));
    assert!(dir.join("meta.db").is_file() && !dir.join("meta.redb").exists());
    assert!(
        dir.join("clean-shutdown").is_file(),
        "nothing was changed before the database was read"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_go_transaction_note_becomes_the_brokers() {
    let dir = go_folder("txn");
    std::fs::write(dir.join("payments-0/transactions"), b"").unwrap();
    std::fs::write(dir.join("payments-0/cleaner-offset"), b"7\n").unwrap();
    take_over(&dir).unwrap().unwrap();
    assert_eq!(
        std::fs::read(dir.join("payments-0/transactional")).unwrap(),
        b"yes\n"
    );
    assert!(
        !dir.join("payments-0/transactions").exists()
            && !dir.join("payments-0/cleaner-offset").exists()
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
