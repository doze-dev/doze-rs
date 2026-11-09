//! A keyed workload against a log, and what has been written.

use std::collections::HashMap;
use std::path::PathBuf;

use kafka_storage::compact::{Cleaned, Params};
use kafka_storage::sim::SimFs;
use kafka_storage::{LogConfig, PartitionLog, Trust};
use kafka_wire::batch::build::{BatchBuilder, Rec};
use kafka_wire::batch::{Batch, Batches, TimestampType};
use kafka_wire::ids::{Offset, TimestampMs};

pub(crate) const NOW: TimestampMs = TimestampMs::from_wire(1_000_000);
pub(crate) const CONFIG: LogConfig = LogConfig {
    segment_bytes: 400,
    index_interval_bytes: 100,
};
/// Cleaning whenever anything is dirty; deletions are kept (their time never comes).
pub(crate) const PARAMS: Params = Params {
    min_dirty_ratio: 0.0,
    min_lag_ms: 0,
    max_lag_ms: i64::MAX,
    delete_retention_ms: i64::MAX / 4,
    now_ms: 10,
    map_bytes: 1 << 20,
};

/// A record as the log shows it: offset, key, whether it is a deletion.
pub(crate) type Seen = (i64, u8, bool);

pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub(crate) fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

pub(crate) fn open(fs: &SimFs, dir: &std::path::Path, trust: Trust) -> PartitionLog<SimFs> {
    PartitionLog::open(fs.clone(), dir.to_path_buf(), CONFIG, trust).unwrap()
}

/// A log with `appends` appends of one to three records over `keys` keys, and every record written down.
pub(crate) fn written(seed: u64, appends: usize, keys: u64) -> (SimFs, PathBuf, Vec<Seen>) {
    let fs = SimFs::new();
    let dir = PathBuf::from("/data/t-0");
    kafka_storage::fs::create_dirs_durably(&fs, &dir).unwrap();
    let mut log = open(&fs, &dir, Trust::Unvouched);
    let mut rng = Rng::new(seed);
    let mut truth = Vec::new();
    for _ in 0..appends {
        let recs: Vec<(u8, bool)> = (0..1 + rng.below(3))
            .map(|_| (u8::try_from(rng.below(keys)).unwrap(), rng.below(4) == 0))
            .collect();
        let first = log.end_offset().get();
        log.append(&[batch(&recs)], NOW, TimestampType::Create)
            .unwrap();
        for (i, (k, t)) in recs.iter().enumerate() {
            truth.push((first + i64::try_from(i).unwrap(), *k, *t));
        }
    }
    log.sync().unwrap();
    (fs, dir, truth)
}

fn batch(recs: &[(u8, bool)]) -> Batch {
    let recs: Vec<Rec> = recs
        .iter()
        .map(|(k, tombstone)| Rec {
            key: Some(vec![*k]),
            value: (!tombstone).then(|| vec![b'v'; 20]),
            ..Rec::default()
        })
        .collect();
    Batch::parse(BatchBuilder::from_records(5, &recs).build()).unwrap()
}

/// Every record the log holds from its start, read the way a consumer does, from the start and then on from where each
/// read left off.
pub(crate) fn records(log: &PartitionLog<SimFs>) -> Vec<Seen> {
    let reader = log.reader();
    let (mut at, end) = (log.start_offset(), log.end_offset());
    let mut out = Vec::new();
    while at < end {
        let chunk = reader.read(at, 1 << 20, None).unwrap();
        assert!(
            !chunk.is_empty(),
            "nothing to read at {at}, before the end {end}"
        );
        for b in Batches::new(chunk) {
            let b = b.unwrap();
            b.each_record(1 << 20, |s| {
                out.push((s.offset, s.key.unwrap()[0], s.tombstone))
            })
            .unwrap();
            at = b.next_offset().unwrap();
        }
    }
    out
}

/// What must hold of the log whatever has been done to it.
pub(crate) fn holds(log: &PartitionLog<SimFs>, truth: &[Seen], what: &str) {
    assert_eq!(
        log.end_offset().get(),
        i64::try_from(truth.len()).unwrap(),
        "{what}: the end moved"
    );
    reads_from_everywhere(log, what);
    let seen = records(log);
    assert!(
        seen.windows(2).all(|w| w[0].0 < w[1].0),
        "{what}: offsets out of order: {seen:?}"
    );
    for r in &seen {
        assert!(truth.contains(r), "{what}: {r:?} was never written");
    }
    let mut latest: HashMap<u8, Seen> = HashMap::new();
    for t in truth {
        latest.insert(t.1, *t);
    }
    for t in latest.values() {
        assert!(
            seen.contains(t),
            "{what}: the latest record of key {} ({t:?}) is gone: {seen:?}",
            t.1
        );
    }
}

/// Rounds, put in place, until the log is no longer due. The last round's reach, if there was one.
pub(crate) fn clean_fully(log: &mut PartitionLog<SimFs>) -> Option<Cleaned> {
    let mut last = None;
    for _ in 0..50 {
        let Some(job) = log
            .clean_job(&PARAMS, log.end_offset(), vec![], HashMap::new())
            .unwrap()
        else {
            return last;
        };
        let cleaned = job.run().unwrap();
        log.install_cleaned(&cleaned).unwrap();
        last = Some(cleaned);
    }
    panic!("cleaning does not finish");
}

pub(crate) fn offset(n: i64) -> Offset {
    Offset::new(n).unwrap()
}

/// The log can be read from every offset in it: the answer begins with the batch that holds the offset, or the next one.
pub(crate) fn reads_from_everywhere(log: &PartitionLog<SimFs>, what: &str) {
    let reader = log.reader();
    for o in log.start_offset().get()..log.end_offset().get() {
        let chunk = reader
            .read(offset(o), 1 << 20, None)
            .unwrap_or_else(|e| panic!("{what}: reading from {o}: {e}"));
        let first = Batches::new(chunk)
            .next()
            .unwrap_or_else(|| panic!("{what}: nothing from {o}"));
        let first = first.unwrap_or_else(|e| panic!("{what}: from {o}: {e}"));
        assert!(
            first.next_offset().unwrap().get() > o,
            "{what}: from {o} the first batch ends before it"
        );
    }
}
