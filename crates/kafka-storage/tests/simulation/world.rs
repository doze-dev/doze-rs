//! A random workload against a partition log on the simulated disk, and what it has been told.

use std::path::PathBuf;

use kafka_storage::sim::SimFs;
use kafka_storage::{LogConfig, PartitionLog, Retention};
use kafka_wire::batch::TimestampType;
use kafka_wire::ids::{Offset, TimestampMs};

use crate::model::{Model, Rec};

pub(crate) const NOW: TimestampMs = TimestampMs::from_wire(1_000_000);
/// Small segments, so the workload rolls often.
pub(crate) const CONFIG: LogConfig = LogConfig {
    segment_bytes: 700,
    index_interval_bytes: 150,
};

/// A small deterministic generator, so a failing run replays from its seed.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub(crate) fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub(crate) fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

pub(crate) struct World {
    pub(crate) fs: SimFs,
    pub(crate) dir: PathBuf,
    /// `None` once the process cannot reopen its log (the disk is halted or failing).
    pub(crate) log: Option<PartitionLog<SimFs>>,
    pub(crate) model: Model,
    /// The steps taken, with the file operations counted after each (for reading a failure).
    pub(crate) trail: Vec<(&'static str, u64)>,
    rng: Rng,
}

pub(crate) fn offset(n: i64) -> Offset {
    Offset::new(n).unwrap()
}

impl World {
    /// A process starting on `fs`, which may already be set to fail or halt. It starts as the broker does: the
    /// clean-stop marker is read and removed and the data directory flushed, then the partition is opened.
    pub(crate) fn new(seed: u64, fs: SimFs) -> Self {
        let dir = PathBuf::from("/data/t-0");
        let log = start(&fs, &dir);
        let mut model = Model::default();
        if let Some(log) = &log {
            model.adopt(log);
        }
        Self {
            fs,
            dir,
            log,
            model,
            trail: Vec::new(),
            rng: Rng::new(seed),
        }
    }

    pub(crate) fn run(&mut self, steps: usize) {
        for _ in 0..steps {
            if self.log.is_none() {
                return;
            }
            self.step();
        }
    }

    fn step(&mut self) {
        let name = match self.rng.below(100) {
            0..=59 => {
                self.append();
                "append"
            }
            60..=74 => {
                self.sync();
                "sync"
            }
            75..=79 => {
                self.restart();
                "restart"
            }
            80..=85 => {
                self.delete_before();
                "delete_before"
            }
            86..=90 => {
                self.retention();
                "retention"
            }
            _ => {
                self.read();
                "read"
            }
        };
        self.trail.push((name, self.fs.ops()));
    }

    fn append(&mut self) {
        let Some(log) = self.log.as_mut() else { return };
        let mut next = log.end_offset().get();
        let mut recs = Vec::new();
        for _ in 0..=self.rng.below(3) {
            let count = 1 + self.rng.below(3) as u32;
            let body =
                vec![self.rng.below(250) as u8; count as usize + self.rng.below(40) as usize];
            recs.push(Rec {
                offset: next,
                count,
                body,
                ts: NOW.get(),
            });
            next += i64::from(count);
        }
        for rec in &recs {
            self.model
                .attempts
                .entry(rec.offset)
                .or_default()
                .push(rec.clone());
        }
        let batches: Vec<_> = recs.iter().map(Rec::produced).collect();
        if log.append(&batches, NOW, TimestampType::Create).is_ok() {
            for rec in recs {
                self.model.acked.insert(rec.offset, rec);
            }
            self.model.end = next;
        }
    }

    fn sync(&mut self) {
        let Some(log) = self.log.as_mut() else { return };
        let covered = log.end_offset().get();
        if log.sync().is_ok() {
            self.model.synced_end = self.model.synced_end.max(covered);
        }
    }

    /// The process is killed (`kill -9`) and started again: the page cache survives, and nothing it holds is
    /// known to be flushed.
    fn restart(&mut self) {
        let fs = self.fs.process_restart();
        self.log = start(&fs, &self.dir);
        self.fs = fs;
        if let Some(log) = &self.log {
            self.model.adopt(log);
        }
    }

    fn delete_before(&mut self) {
        let Some(log) = self.log.as_mut() else { return };
        let (start, end) = (log.start_offset().get(), log.end_offset().get());
        let target = start + self.rng.below((end - start) as u64 + 1) as i64;
        let to = (self.rng.below(5) != 0).then(|| offset(target));
        let reaches = to.map_or(end, Offset::get);
        if log.delete_before(to).is_ok() && reaches > start {
            self.model.durable_start = self.model.durable_start.max(reaches);
        }
    }

    fn retention(&mut self) {
        let Some(log) = self.log.as_mut() else { return };
        let policy = Retention {
            max_bytes: Some(200 + self.rng.below(1500)),
            max_age_ms: None,
        };
        let _ = log.enforce_retention(policy, NOW);
    }

    fn read(&mut self) {
        let Some(log) = self.log.as_ref() else { return };
        let (start, end) = (log.start_offset().get(), log.end_offset().get());
        let from = start + self.rng.below((end - start) as u64 + 1) as i64;
        if let Ok(got) = log
            .reader()
            .read(offset(from), 1 + self.rng.below(400) as usize, None)
        {
            for batch in kafka_wire::batch::Batches::new(got) {
                let base = batch.unwrap().base_offset().unwrap().get();
                assert!(
                    base >= start.min(from).saturating_sub(5),
                    "a read returned {base} for {from}"
                );
            }
        }
    }
}

/// What starting the broker does before it serves a partition. `None` if the disk does not let it.
fn start(fs: &SimFs, dir: &std::path::Path) -> Option<PartitionLog<SimFs>> {
    let data = dir.parent()?;
    let state = kafka_storage::marker::StopState::begin(fs, data).ok()?;
    let trust = state.trust("t-0");
    PartitionLog::open(fs.clone(), dir.to_path_buf(), CONFIG, trust).ok()
}
