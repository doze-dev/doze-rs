use std::path::{Path, PathBuf};

use super::*;
use crate::fs::FsFile;
use crate::sim::SimFs;

fn file(fs: &SimFs, path: &Path, len: usize) {
    let (f, _) = fs.open_or_create(path).unwrap();
    f.write_all_at(&vec![7; len], 0).unwrap();
}

fn data() -> (SimFs, PathBuf) {
    let fs = SimFs::new();
    let data = PathBuf::from("/data");
    fs.create_dir_all(&data).unwrap();
    (fs, data)
}

#[test]
fn a_partition_name_is_a_topic_a_dash_and_digits() {
    assert_eq!(partition_of("orders-0"), Some(("orders", 0)));
    assert_eq!(partition_of("a-b-12"), Some(("a-b", 12)));
    for not in [
        "orders",
        "orders-",
        "-3",
        "orders-x",
        "orders-1a",
        "meta.redb",
    ] {
        assert_eq!(partition_of(not), None, "{not}");
    }
}

#[test]
fn every_byte_is_counted_once_under_the_kind_it_belongs_to() {
    let (fs, data) = data();
    for (dir, files) in [
        (
            "orders-0",
            &[
                ("00000000000000000000.log", 100),
                ("00000000000000000000.index", 8),
            ][..],
        ),
        (
            "orders-1",
            &[
                ("00000000000000000000.log", 40),
                ("00000000000000000005.log", 60),
                ("log-start", 3),
            ][..],
        ),
        ("gone-0", &[("00000000000000000000.log", 9)][..]),
    ] {
        fs.create_dir(&data.join(dir)).unwrap();
        for (name, len) in files {
            file(&fs, &data.join(dir).join(name), *len);
        }
    }
    file(&fs, &data.join("meta.redb"), 500);
    file(&fs, &data.join("lock"), 0);
    file(&fs, &data.join("clean-stop"), 12);
    fs.create_dir(&data.join("notes")).unwrap();
    file(&fs, &data.join("notes/a.txt"), 30);
    let u = walk_data_dir(&fs, &data).unwrap();
    let summary: Vec<_> = u
        .partitions
        .iter()
        .map(|p| (p.topic.as_str(), p.partition, p.bytes, p.files, p.segments))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("gone", 0, 9, 1, 1),
            ("orders", 0, 108, 2, 1),
            ("orders", 1, 103, 3, 2)
        ]
    );
    assert!(u.partitions.iter().all(|p| p.last_write_ms.is_some()));
    // A partition's size as Kafka gives it is its log files alone.
    let logs: Vec<u64> = u.partitions.iter().map(|p| p.log_bytes).collect();
    assert_eq!(logs, vec![9, 100, 100]);
    assert_eq!(u.meta_bytes, 500);
    assert_eq!((u.other_bytes, u.other_files), (42, 3));
    assert_eq!(u.total(), 9 + 108 + 103 + 500 + 42);
    assert!(!u.cut);
}

#[test]
fn a_data_directory_that_is_not_there_holds_nothing() {
    let (fs, data) = data();
    assert_eq!(
        walk_data_dir(&fs, &data.join("nowhere")).unwrap(),
        DirUsage::default()
    );
}
