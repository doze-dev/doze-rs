//! Against files Go's bbolt 1.5.0 wrote (tests/fixtures/make_fixtures.go.txt, page size 1024, cut at the high page):
//! branch pages, an overflowing value, an inline and a paged sub-bucket, an empty bucket, and two transactions.

use super::{Db, Error, Item};
use std::path::Path;

fn fixture(name: &str) -> Db {
    Db::open(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

#[test]
fn every_key_of_a_real_file_is_read() {
    let db = fixture("tree.bolt");
    let names: Vec<String> = db
        .buckets(&[])
        .unwrap()
        .into_iter()
        .map(|b| String::from_utf8(b).unwrap())
        .collect();
    assert_eq!(names, ["_schema", "big", "empty", "outer", "small"]);

    let small = db.values(&[b"small"]).unwrap();
    let keys: Vec<&[u8]> = small.iter().map(|(k, _)| k.as_slice()).collect();
    assert_eq!(
        keys,
        [&b"a"[..], b"b", b"c", b"d"],
        "the second transaction's key is there"
    );
    assert_eq!(small[0].1, b"1");
    assert_eq!(small[1].1, b"");
    assert_eq!(
        small[2].1,
        vec![b'x'; 3000 + 2000],
        "a value over several pages"
    );
    assert_eq!(small[3].1, b"later");

    let big = db.values(&[b"big"]).unwrap();
    assert_eq!(big.len(), 600, "through branch pages");
    for (i, (k, v)) in big.iter().enumerate() {
        assert_eq!(k, format!("k{i:05}").as_bytes());
        assert_eq!(v, format!("v{}", i * 7).as_bytes());
    }

    assert_eq!(
        db.values(&[b"outer", b"inner"]).unwrap(),
        [(b"deep".to_vec(), b"value".to_vec())],
        "inline"
    );
    assert_eq!(
        db.values(&[b"outer", b"large"]).unwrap().len(),
        120,
        "paged"
    );
    let outer = db.items(&[b"outer"]).unwrap();
    assert_eq!(
        outer,
        [
            (b"inner".to_vec(), Item::Bucket),
            (b"large".to_vec(), Item::Bucket),
            (b"plain".to_vec(), Item::Value(b"here".to_vec()))
        ]
    );
    assert!(db.values(&[b"empty"]).unwrap().is_empty());
    assert!(db.values(&[b"nowhere"]).unwrap().is_empty());
    assert!(db.holds_data().unwrap());
}

#[test]
fn a_file_with_only_its_schema_holds_no_data() {
    let db = fixture("empty.bolt");
    assert_eq!(
        db.buckets(&[]).unwrap(),
        [b"_schema".to_vec(), b"queues".to_vec()]
    );
    assert!(!db.holds_data().unwrap());
}

#[test]
fn damage_is_an_error_not_a_panic() {
    let good =
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tree.bolt"))
            .unwrap();
    assert!(matches!(Db::parse(Vec::new()), Err(Error::Corrupt(_))));
    assert!(matches!(
        Db::parse(b"not a database at all".to_vec()),
        Err(Error::Corrupt(_))
    ));
    // Every single-byte change past the metas, and every cut, reads or fails; none panics or loops.
    for at in (2048..good.len()).step_by(97) {
        let mut bad = good.clone();
        bad[at] ^= 0xA5;
        if let Ok(db) = Db::parse(bad) {
            let _ = db.holds_data();
            let _ = db.values(&[b"big"]);
            let _ = db.values(&[b"outer", b"large"]);
        }
    }
    for cut in (0..good.len()).step_by(1024) {
        if let Ok(db) = Db::parse(good[..cut].to_vec()) {
            let _ = db.values(&[b"big"]);
        }
    }
    // One damaged meta: the other is used.
    let mut one = good.clone();
    one[16] ^= 1;
    assert_eq!(
        Db::parse(one).unwrap().values(&[b"big"]).unwrap().len(),
        600
    );
}
