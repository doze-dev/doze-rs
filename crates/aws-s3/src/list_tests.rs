//! ListObjects folding: a prefix under a delimiter is read as one row, not all its keys.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Instant;

use crate::core::Core;
use crate::list::{Entry, Page, Query};
use crate::model::Version;
use crate::model::{Bucket, Lock, Meta};

fn row(key: &str, seq: u64) -> (String, Vec<u8>) {
    let v = Version {
        key: key.into(),
        version_id: "null".into(),
        seq,
        delete_marker: false,
        modified_ms: 1,
        size: 1,
        etag: "e".into(),
        meta: Meta::default(),
        checksum: None,
        segs: Vec::new(),
        parts: Vec::new(),
        tags: Vec::new(),
        lock: Lock::default(),
        acl: None,
    };
    (
        crate::store::row_key("bkt", key, seq),
        doze_store::record::encode("version", crate::core::ROW_VERSION, &v).unwrap(),
    )
}

fn names(page: &Page) -> Vec<String> {
    page.entries
        .iter()
        .map(|e| match e {
            Entry::Object(v) => v.key.clone(),
            Entry::Prefix(p) => p.clone(),
        })
        .collect()
}

/// Before: every key under a folded prefix was read (100,000 rows for `deep/`). After: a few, then the walk starts past it.
#[tokio::test]
async fn a_folded_prefix_is_read_as_one_row_not_all_its_keys() {
    let dir = std::env::temp_dir().join(format!("aws-s3-list-fold-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    let clock = Arc::new(doze_runtime::SystemClock);
    let mut core = Core::open_in(&dir, clock, None).unwrap();
    core.buckets.insert("bkt".into(), Bucket::default());
    let mut seq = 0;
    let mut put = |core: &mut Core, key: &str| {
        seq += 1;
        core.batch.puts.push(row(key, seq));
    };
    put(&mut core, "a.txt");
    for n in 0..100_000 {
        put(&mut core, &format!("deep/{n:06}"));
    }
    // A key right after the fold point, and a short fold.
    put(&mut core, "deep/\u{10FFFF}odd");
    put(&mut core, "deep0");
    put(&mut core, "x/1");
    put(&mut core, "x/2");
    if let Some(ack) = core.flush().unwrap() {
        ack.await.unwrap();
    }
    let q = |after: &str, max: usize| -> Page {
        let q = Query {
            bucket: "bkt",
            prefix: "",
            delimiter: Some("/"),
            after,
            max,
        };
        core.page(&q).unwrap()
    };
    let took = Instant::now();
    let page = q("", 1000);
    let took = took.elapsed();
    assert_eq!(names(&page), ["a.txt", "deep/", "deep0", "x/"]);
    assert_eq!(page.next, None);
    assert!(page.scanned < 100, "{} rows read in {took:?}", page.scanned);
    // Paged one entry at a time, a fold is not returned twice.
    let mut after = String::new();
    let mut seen = Vec::new();
    loop {
        let page = q(&after, 1);
        seen.extend(names(&page));
        assert!(
            page.scanned < 100,
            "{} rows read after {after:?}",
            page.scanned
        );
        match page.next {
            Some(n) => after = n,
            None => break,
        }
    }
    assert_eq!(seen, ["a.txt", "deep/", "deep0", "x/"]);
    core.db.close();
    drop(std::fs::remove_dir_all(&dir));
}

/// ListObjectVersions under a delimiter: before, every version under a folded prefix was read (100,005 rows for `deep/`); after, a
/// few, then the walk starts past the fold. Paged one entry at a time, a fold is not returned twice.
#[tokio::test]
async fn a_folded_prefix_of_versions_is_read_as_one_row_not_all_its_keys() {
    let dir = std::env::temp_dir().join(format!("aws-s3-listv-fold-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    let clock = Arc::new(doze_runtime::SystemClock);
    let mut core = Core::open_in(&dir, clock, None).unwrap();
    core.buckets.insert("bkt".into(), Bucket::default());
    let mut seq = 0;
    let mut put = |core: &mut Core, key: &str| {
        seq += 1;
        core.batch.puts.push(row(key, seq));
    };
    put(&mut core, "a.txt");
    for n in 0..100_000 {
        put(&mut core, &format!("deep/{n:06}"));
    }
    put(&mut core, "deep/\u{10FFFF}odd");
    put(&mut core, "deep0");
    put(&mut core, "x/1");
    put(&mut core, "x/2");
    if let Some(ack) = core.flush().unwrap() {
        ack.await.unwrap();
    }
    let list = |after: &str, max: usize| {
        let q = Query {
            bucket: "bkt",
            prefix: "",
            delimiter: Some("/"),
            after,
            max,
        };
        core.collect(&q, None, crate::list::Enc(false), "000000000000")
            .unwrap()
    };
    let took = Instant::now();
    let found = list("", 1000);
    let took = took.elapsed();
    let keys: Vec<&str> = found
        .versions
        .iter()
        .filter_map(|v| v["Key"].as_str())
        .collect();
    assert_eq!(keys, ["a.txt", "deep0"]);
    assert_eq!(found.prefixes, ["deep/", "x/"]);
    assert!(
        found.scanned < 100,
        "{} rows read in {took:?}",
        found.scanned
    );
    core.db.close();
    drop(std::fs::remove_dir_all(&dir));
}
