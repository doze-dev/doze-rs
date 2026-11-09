//! A random workload against a partition log on the simulated disk, and what it has been told.

use std::path::PathBuf;

use kafka_storage::sim::{Cut, DataCut, NameCut, SimFs};
use kafka_storage::{LogConfig, PartitionLog, Retention};
use kafka_wire::batch::TimestampType;
use kafka_wire::ids::{Offset, TimestampMs};

use crate::model::{Model, Rec};
use crate::rng::Rng;

pub(crate) const NOW: TimestampMs = TimestampMs::from_wire(1_000_000);
/// Small segments, so the workload rolls often.
pub(crate) const CONFIG: LogConfig = LogConfig {
    segment_bytes: 700,
    index_interval_bytes: 150,
    segment_ms: None,
};

pub(crate) struct World {
    pub(crate) fs: SimFs,
    pub(crate) dir: PathBuf,
    /// `None` once the process cannot reopen its log (the disk is halted or failing).
    pub(crate) log: Option<PartitionLog<SimFs>>,
    pub(crate) model: Model,
    /// The steps taken, with the file operations counted after each (for reading a failure).
    pub(crate) trail: Vec<(&'static str, u64)>,
    /// The file operations of every earlier process, oldest first.
    pub(crate) earlier: Vec<Vec<(kafka_storage::sim::Op, std::path::PathBuf)>>,
    /// The seed this world was made from.
    pub(crate) seed: u64,
    pub(crate) rng: Rng,
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
            earlier: Vec::new(),
            seed,
            rng: Rng::new(seed),
        }
    }

    pub(crate) fn run(&mut self, steps: usize) {
        for _ in 0..steps {
            if self.log.is_none() {
                return;
            }
            self.step();
            if std::env::var_os("SIM_TRACE_FILE").is_some() {
                let p = self
                    .dir
                    .join(std::env::var("SIM_TRACE_FILE").unwrap_or_default());
                let (v, d) = (
                    self.fs.contents(&p).map(|c| c.len()),
                    self.fs.durable_contents(&p).map(|c| c.len()),
                );
                println!(
                    "after {:?}: {} live={v:?} durable={d:?}",
                    self.trail.last().map(|t| t.0),
                    self.trail.len()
                );
            }
            self.check_live();
        }
    }

    /// The running process's own log must agree with what it has been told, after every step: a problem is
    /// caught at the step that made it, not at the power cut that happens to expose it.
    fn check_live(&self) {
        let Some(log) = &self.log else { return };
        if let Err(e) = crate::model::verify(log, &self.model, false) {
            if e.contains("the process was stopped") {
                return; // a stopped process cannot be asked
            }
            let steps: Vec<_> = self.trail.iter().rev().take(8).collect();
            panic!(
                "seed {}: the live log is inconsistent after {steps:?}: {e}",
                self.seed
            );
        }
    }

    fn step(&mut self) {
        let name = match self.rng.below(100) {
            0..=56 => {
                self.append();
                "append"
            }
            57..=71 => {
                self.sync();
                "sync"
            }
            72..=75 => {
                self.restart();
                "restart"
            }
            76..=78 => {
                self.clean_stop();
                "clean_stop"
            }
            79..=84 => {
                self.delete_before();
                "delete_before"
            }
            85..=89 => {
                self.retention();
                "retention"
            }
            _ => {
                self.read();
                "read"
            }
        };
        if let Some(log) = &self.log {
            self.model.start_seen = self.model.start_seen.max(log.start_offset().get());
        }
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

    /// The process stops cleanly (flushes and closes the log, records the marker) and starts again, so the
    /// next open is vouched for.
    fn clean_stop(&mut self) {
        let Some(log) = self.log.take() else { return };
        let end = log.end_offset().get();
        let data = self.dir.parent().map(std::path::Path::to_path_buf);
        let closed = log.close();
        // A close that fails is a flush that failed: the log went offline on the way out.
        let offline = closed.is_err();
        let stopped = closed.is_ok()
            && data.is_some_and(|d| {
                kafka_storage::marker::record_clean_stop(&self.fs, &d, &[]).is_ok()
            });
        if stopped {
            self.model.synced_end = self.model.synced_end.max(end);
        }
        self.start_again(offline);
    }

    /// The process is killed (`kill -9`) and started again: the page cache survives, and nothing it holds is
    /// known to be flushed.
    fn restart(&mut self) {
        let offline = self.log.as_ref().is_some_and(|l| l.is_offline());
        self.start_again(offline);
    }

    /// A new process on the same disk. If the old one's log had gone offline because a flush failed, the new
    /// one is not started on the same page cache: after a failed fsync the kernel has marked the dropped pages
    /// clean, so a later fsync of them reports success and writes nothing, and nothing a process can do tells
    /// the two apart. Only the machine starting again clears it, so that is what an offline log's restart is.
    fn start_again(&mut self, after_failed_flush: bool) {
        let fs = if after_failed_flush {
            let seed = self.rng.next();
            self.fs.power_cut(Cut {
                data: DataCut::Pages,
                names: NameCut::Random,
                seed,
            })
        } else {
            self.fs.process_restart()
        };
        self.log = start(&fs, &self.dir);
        self.earlier.push(self.fs.trace());
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
        // The start is recorded durably before the call returns, so a call that then fails may still have moved it.
        self.model.start_seen = self.model.start_seen.max(reaches);
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
        // Retention records the start it is moving to before it removes anything, so a call that then fails
        // may have moved it to the first segment it would have kept: at most the active segment's base.
        let active_base = self
            .fs
            .names(&self.dir)
            .iter()
            .filter_map(|n| n.strip_suffix(".log")?.parse::<i64>().ok())
            .max()
            .unwrap_or(0);
        self.model.start_seen = self.model.start_seen.max(active_base);
        let _ = log.enforce_retention(policy, NOW);
    }
}

/// What starting the broker does before it serves a partition. `None` if the disk does not let it.
fn start(fs: &SimFs, dir: &std::path::Path) -> Option<PartitionLog<SimFs>> {
    let data = dir.parent()?;
    let state = kafka_storage::marker::StopState::begin(fs, data).ok()?;
    let trust = state.trust("t-0");
    PartitionLog::open(fs.clone(), dir.to_path_buf(), CONFIG, trust).ok()
}
