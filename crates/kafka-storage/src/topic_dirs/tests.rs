use super::*;
use crate::fs::FsFile;
use crate::sim::{Cut, DataCut, NameCut, SimFs};

const DATA: &str = "/data";

fn disk() -> SimFs {
    let fs = SimFs::new();
    fs.create_dir(Path::new(DATA)).unwrap();
    fs.sync_dir(Path::new("/")).unwrap();
    fs
}

fn topic(n: &str) -> TopicName {
    TopicName::parse(n).unwrap()
}

fn dirs(fs: &SimFs) -> Vec<String> {
    let mut names = fs.names(Path::new(DATA));
    names.sort();
    names
}

fn after_power_cut(fs: &SimFs) -> SimFs {
    fs.power_cut(Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 1,
    })
}

#[test]
fn partitions_are_directories_named_for_the_topic_and_number_and_made_durably() {
    let fs = disk();
    create_partition_dirs(
        &fs,
        Path::new(DATA),
        &topic("t"),
        PartitionCount::new(3).unwrap(),
    )
    .unwrap();
    assert_eq!(dirs(&after_power_cut(&fs)), ["t-0", "t-1", "t-2"]);
}

#[test]
fn removing_a_topic_takes_its_files_and_directories_and_nothing_else_durably() {
    let fs = disk();
    let data = Path::new(DATA);
    for t in ["t", "t-1", "u"] {
        create_partition_dirs(&fs, data, &topic(t), PartitionCount::new(2).unwrap()).unwrap();
    }
    let (file, _) = fs
        .open_or_create(&data.join("t-1").join("00000000000000000000.log"))
        .unwrap();
    file.write_all_at(b"records", 0).unwrap();
    remove_topic_dirs(&fs, data, &topic("t"), PartitionCount::new(2).unwrap()).unwrap();
    // Topic "t-1"'s partitions are named like partition 1 of "t" ("t-1-0" is not "t-1"), and are untouched.
    let expected = ["t-1-0", "t-1-1", "u-0", "u-1"];
    assert_eq!(dirs(&fs), expected);
    assert_eq!(dirs(&after_power_cut(&fs)), expected);
}

#[test]
fn only_directories_that_look_like_partitions_of_the_name_are_stray() {
    let fs = disk();
    let data = Path::new(DATA);
    for d in ["t-0", "t-12", "t-x", "t-", "t", "tt-0", "u-0"] {
        fs.create_dir(&data.join(d)).unwrap();
    }
    fs.open_or_create(&data.join("t-5")).unwrap();
    assert_eq!(remove_stray_dirs(&fs, data, &topic("t")).unwrap(), 2);
    assert_eq!(dirs(&fs), ["t", "t-", "t-5", "t-x", "tt-0", "u-0"]);
}
