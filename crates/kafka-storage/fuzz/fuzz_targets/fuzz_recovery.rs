#![no_main]
//! Any bytes as a segment's log and its index: opening must not panic, must produce a log that is consistent
//! (what it serves is whole batches, one after another, up to its end), and recovering it again must change
//! nothing. The first byte says where the log ends and the index begins.

use std::path::{Path, PathBuf};

use kafka_storage::fs::{Fs, FsFile};
use kafka_storage::sim::SimFs;
use kafka_storage::{LogConfig, PartitionLog, Trust};
use kafka_wire::batch::{Batches, TimestampType};
use kafka_wire::ids::TimestampMs;
use libfuzzer_sys::fuzz_target;

const CONFIG: LogConfig = LogConfig {
    segment_bytes: 1 << 20,
    index_interval_bytes: 256,
};

fn disk(log: &[u8], index: &[u8]) -> (SimFs, PathBuf) {
    let fs = SimFs::new();
    let dir = PathBuf::from("/data/t-0");
    fs.create_dir_all(&dir).unwrap();
    for (name, bytes) in [("00000000000000000000.log", log), ("00000000000000000000.index", index)] {
        let (file, _) = fs.open_or_create(&dir.join(name)).unwrap();
        file.write_all_at(bytes, 0).unwrap();
    }
    (fs, dir)
}

fn served(log: &PartitionLog<SimFs>) -> Vec<u8> {
    let reader = log.reader();
    let (mut from, end) = (reader.start_offset(), reader.end_offset());
    let mut out = Vec::new();
    while from < end {
        let got = reader.read(from, usize::MAX, None).unwrap();
        let mut any = false;
        for batch in Batches::new(got.clone()) {
            let batch = batch.expect("a recovered log serves only whole, valid batches");
            from = batch.base_offset().unwrap().checked_add(batch.record_count()).unwrap();
            any = true;
        }
        assert!(any, "a hole before the end");
        out.extend_from_slice(&got);
    }
    out
}

fuzz_target!(|data: &[u8]| {
    let Some((split, rest)) = data.split_first() else { return };
    let at = (usize::from(*split) * rest.len()) / 256;
    let (log, index) = rest.split_at(at);
    let (fs, dir) = disk(log, index);
    let Ok(mut opened) = PartitionLog::open(fs.clone(), dir.clone(), CONFIG, Trust::Unvouched) else { return };
    let first = served(&opened);
    let end = opened.end_offset();
    opened.append(&[], TimestampMs::from_wire(0), TimestampType::Create).unwrap();
    drop(opened);
    // Recovering what recovery left changes nothing.
    let again = PartitionLog::open(fs.process_restart(), Path::new(&dir).to_path_buf(), CONFIG, Trust::Unvouched).unwrap();
    assert_eq!(again.end_offset(), end);
    assert_eq!(served(&again), first);
});
