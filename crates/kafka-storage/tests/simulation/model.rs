//! What a workload has been told, and the check that a reopened log is consistent with it.

use std::collections::BTreeMap;

use bytes::Bytes;
use kafka_storage::PartitionLog;
use kafka_storage::sim::SimFs;
use kafka_wire::batch::build::BatchBuilder;
use kafka_wire::batch::{Batch, Batches};

/// One batch as the workload produced it, and where the log put it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rec {
    pub(crate) offset: i64,
    pub(crate) count: u32,
    pub(crate) body: Vec<u8>,
    pub(crate) ts: i64,
}

impl Rec {
    /// The batch as a producer sends it (base offset 0).
    pub(crate) fn produced(&self) -> Batch {
        Batch::parse(self.stamped(0)).unwrap()
    }

    /// The bytes the log must hold for it: stamped with its offset.
    pub(crate) fn stamped(&self, base: i64) -> Bytes {
        BatchBuilder::new()
            .base_offset(base)
            .records(self.count)
            .body(self.body.clone())
            .timestamps(self.ts, self.ts)
            .build()
    }
}

/// What the workload knows.
#[derive(Debug, Default)]
pub(crate) struct Model {
    /// Batches whose append returned Ok, by offset.
    pub(crate) acked: BTreeMap<i64, Rec>,
    /// Every batch that was tried at an offset, acknowledged or not: a call that failed may still have left
    /// a whole batch behind (the process stopped between the write and the take-back), and a crash may
    /// find it.
    pub(crate) attempts: BTreeMap<i64, Vec<Rec>>,
    /// The end offset at the last sync that returned Ok: everything below is on disk.
    pub(crate) synced_end: i64,
    /// The start recorded by a DeleteBefore that returned Ok: durable.
    pub(crate) durable_start: i64,
    /// The end offset the workload last saw.
    pub(crate) end: i64,
}

/// Checks a log (reopened after a stop or a cut) against the model:
///
/// - it holds at least everything a flush had acknowledged, in order, byte for byte;
/// - what it holds beyond that is a prefix of what was tried, never a hole or a stranger;
/// - its start is no earlier than any recorded start;
/// - with `strict`, it holds nothing that was not acknowledged.
pub(crate) fn verify(log: &PartitionLog<SimFs>, m: &Model, strict: bool) -> Result<(), String> {
    let reader = log.reader();
    let (start, end) = (reader.start_offset().get(), reader.end_offset().get());
    if start < m.durable_start.min(end) {
        return Err(format!(
            "start {start} is before the recorded start {}",
            m.durable_start
        ));
    }
    if start > end {
        return Err(format!("start {start} is past the end {end}"));
    }
    if end < m.synced_end {
        return Err(format!(
            "end {end} lost records below {}, which a flush had acknowledged",
            m.synced_end
        ));
    }
    if strict && end > m.end {
        return Err(format!(
            "end {end} holds records past the {} that were acknowledged",
            m.end
        ));
    }
    let mut from = start;
    while from < end {
        let got = reader
            .read(
                kafka_wire::ids::Offset::new(from).unwrap(),
                usize::MAX,
                None,
            )
            .map_err(|e| format!("reading {from}: {e}"))?;
        if got.is_empty() {
            return Err(format!("a hole: nothing at {from}, end is {end}"));
        }
        for batch in Batches::new(got) {
            let batch = batch.map_err(|e| format!("a damaged batch is served at {from}: {e}"))?;
            let base = batch.base_offset().unwrap().get();
            // The log starts mid-batch if DeleteRecords put the start inside one: the batch holding the start
            // is served whole, as Kafka serves it, and the client skips the records before its offset.
            let holds_start =
                from == start && base <= from && from < base + i64::from(batch.record_count());
            if base != from && !holds_start {
                return Err(format!("expected a batch at {from}, found {base}"));
            }
            check_content(m, base, batch.as_bytes())?;
            from = base + i64::from(batch.record_count());
        }
    }
    Ok(())
}

fn check_content(m: &Model, base: i64, bytes: &Bytes) -> Result<(), String> {
    let matches = |r: &Rec| r.stamped(base) == *bytes;
    let ok = if base < m.synced_end {
        m.acked.get(&base).is_some_and(matches)
    } else {
        m.attempts
            .get(&base)
            .is_some_and(|rs| rs.iter().any(matches))
    };
    if ok {
        return Ok(());
    }
    let tried: Vec<_> = m
        .attempts
        .get(&base)
        .into_iter()
        .flatten()
        .map(|r| (r.count, r.body.len()))
        .collect();
    let acked = m.acked.get(&base).map(|r| (r.count, r.body.len()));
    let found = Batch::parse(bytes.clone()).map(|b| (b.record_count(), bytes.len()));
    Err(format!(
        "the batch at {base} is not one the workload wrote there: found (records, bytes) {found:?}; \
         acknowledged (records, body) {acked:?}; tried {tried:?}; synced below {}",
        m.synced_end
    ))
}

/// Every batch the log serves from its start, with the offset it sits at.
pub(crate) fn contents(log: &PartitionLog<SimFs>) -> Vec<(i64, Bytes)> {
    let reader = log.reader();
    let (mut from, end) = (reader.start_offset().get(), reader.end_offset().get());
    let mut out = Vec::new();
    while from < end {
        let got = reader
            .read(
                kafka_wire::ids::Offset::new(from).unwrap(),
                usize::MAX,
                None,
            )
            .unwrap();
        if got.is_empty() {
            break;
        }
        for batch in Batches::new(got).map_while(Result::ok) {
            let base = batch.base_offset().unwrap().get();
            from = base + i64::from(batch.record_count());
            out.push((base, batch.as_bytes().clone()));
        }
    }
    out
}

impl Model {
    /// A log was reopened. It may hold whole batches that a call reported as failed, because the process
    /// stopped between writing one and taking it back; recovery cannot tell them from the rest, so they are
    /// the log's now, and what the workload knows grows to include them.
    pub(crate) fn adopt(&mut self, log: &PartitionLog<SimFs>) {
        for (base, bytes) in contents(log) {
            if self.acked.contains_key(&base) {
                continue;
            }
            let found = self
                .attempts
                .get(&base)
                .and_then(|rs| rs.iter().find(|r| r.stamped(base) == bytes));
            if let Some(rec) = found.cloned() {
                self.acked.insert(base, rec);
            }
        }
        self.end = self.end.max(log.end_offset().get());
    }
}
