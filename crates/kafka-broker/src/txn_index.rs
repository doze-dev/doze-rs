//! What a partition knows of its transactions: which are open (a producer's first offset in the transaction) and which
//! were aborted (first offset and the offset of the marker). From these come the **last stable offset**, the first
//! offset of the oldest open transaction or the end of the log if none is open, below which a `read_committed` consumer
//! may read, and the aborted transactions that touch what it is given, so that it skips their records.
//!
//! This is pure. The partition's writer feeds it every transactional batch and every marker as it is appended, and
//! rebuilds it from the log after a restart.

use std::collections::BTreeMap;

use kafka_wire::batch::Marker;

/// A transaction that was aborted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Aborted {
    pub(crate) producer: i64,
    /// The offset of its first record in this partition.
    pub(crate) first: i64,
    /// The offset of its abort marker.
    pub(crate) last: i64,
}

/// A transaction that is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Open {
    pub(crate) producer: i64,
    pub(crate) epoch: i16,
    pub(crate) first: i64,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct TxnIndex {
    open: BTreeMap<i64, Open>,
    /// By first offset.
    aborted: Vec<Aborted>,
}

impl TxnIndex {
    /// A transactional batch from `producer` was written at `offset`: it opens the producer's transaction if none is.
    pub(crate) fn write(&mut self, producer: i64, epoch: i16, offset: i64) {
        self.open.entry(producer).or_insert(Open {
            producer,
            epoch,
            first: offset,
        });
    }

    /// A marker for `producer`'s transaction was written at `offset`. A marker for a producer with no open transaction
    /// here (it wrote nothing, or the marker is written again after a crash) changes nothing.
    pub(crate) fn marker(&mut self, producer: i64, offset: i64, kind: Marker) {
        let Some(open) = self.open.remove(&producer) else {
            return;
        };
        if kind == Marker::Abort {
            let at = self.aborted.partition_point(|a| a.first <= open.first);
            self.aborted.insert(
                at,
                Aborted {
                    producer,
                    first: open.first,
                    last: offset,
                },
            );
        }
    }

    /// The first offset that is not stable: the oldest open transaction's first, or `end` if none is open.
    pub(crate) fn last_stable(&self, end: i64) -> i64 {
        self.open
            .values()
            .map(|o| o.first)
            .min()
            .map_or(end, |first| first.min(end))
    }

    /// The aborted transactions that touch `from..to`: ended at or after `from` and begun before `to`.
    pub(crate) fn aborted_between(&self, from: i64, to: i64) -> Vec<Aborted> {
        self.aborted
            .iter()
            .filter(|a| a.last >= from && a.first < to)
            .copied()
            .collect()
    }

    /// The open transactions.
    pub(crate) fn open(&self) -> Vec<Open> {
        self.open.values().copied().collect()
    }

    /// The transaction `producer` has open here, if any.
    #[expect(dead_code, reason = "the coordinator, in slice 6d")]
    pub(crate) fn open_of(&self, producer: i64) -> Option<Open> {
        self.open.get(&producer).copied()
    }

    /// The log's start moved up: aborted transactions that ended before it are not needed.
    pub(crate) fn forget_before(&mut self, start: i64) {
        self.aborted.retain(|a| a.last >= start);
    }
}

#[cfg(test)]
mod tests;
