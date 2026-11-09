//! What cleaning knows of transactions as it goes through the log in order (source: `CleanedTransactionMetadata`): the
//! records of an aborted transaction are dropped, and its marker is kept until none is left to drop; the marker of a
//! committed transaction goes once no record of it is seen.

use std::collections::{HashMap, HashSet};

/// An aborted transaction: the producer, and the offsets from its first record to its marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbortedTxn {
    pub producer: i64,
    pub first: i64,
    pub last: i64,
}

#[derive(Debug, Default)]
pub(crate) struct CleanedTxns {
    /// Aborted transactions not yet reached, by first offset.
    ahead: Vec<AbortedTxn>,
    committed: HashSet<i64>,
    /// Aborted transactions reached and not yet closed: whether a record of one has been dropped.
    aborted: HashMap<i64, bool>,
}

impl CleanedTxns {
    pub(crate) fn new(mut aborted: Vec<AbortedTxn>) -> Self {
        aborted.sort_by_key(|t| std::cmp::Reverse(t.first));
        Self {
            ahead: aborted,
            ..Self::default()
        }
    }

    fn reach(&mut self, offset: i64) {
        while self.ahead.last().is_some_and(|t| t.first <= offset) {
            if let Some(t) = self.ahead.pop() {
                self.aborted.entry(t.producer).or_insert(false);
            }
        }
    }

    /// A data batch ending at `last_offset` is read. Whether it belongs to an aborted transaction and so is dropped.
    pub(crate) fn on_batch(
        &mut self,
        producer: i64,
        transactional: bool,
        last_offset: i64,
    ) -> bool {
        self.reach(last_offset);
        if !transactional {
            return false;
        }
        if let Some(dropped) = self.aborted.get_mut(&producer) {
            *dropped = true;
            true
        } else {
            self.committed.insert(producer);
            false
        }
    }

    /// A marker is read, committing (`true`) or aborting. Whether it may go.
    pub(crate) fn on_marker(
        &mut self,
        producer: i64,
        commit: Option<bool>,
        last_offset: i64,
    ) -> bool {
        self.reach(last_offset);
        match commit {
            // Kept until every record of the transaction it aborts has been dropped in an earlier round.
            Some(false) => self
                .aborted
                .remove(&producer)
                .is_none_or(|dropped| !dropped),
            // Kept if a record of the transaction was seen: the reader of the log still needs to know it committed.
            Some(true) => !self.committed.remove(&producer),
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(producer: i64, first: i64, last: i64) -> AbortedTxn {
        AbortedTxn {
            producer,
            first,
            last,
        }
    }

    #[test]
    fn the_records_of_an_aborted_transaction_go_and_its_marker_stays_until_none_is_left_to_drop() {
        let mut c = CleanedTxns::new(vec![t(7, 10, 20)]);
        assert!(!c.on_batch(7, true, 5), "before the transaction began");
        assert!(c.on_batch(7, true, 12), "inside it");
        assert!(!c.on_batch(8, true, 13), "another producer's");
        assert!(
            !c.on_marker(7, Some(false), 20),
            "a record was dropped this round: the marker stays"
        );
        // A round later nothing of it is left to drop: the marker may go.
        let mut later = CleanedTxns::new(vec![t(7, 10, 20)]);
        assert!(later.on_marker(7, Some(false), 20));
    }

    #[test]
    fn a_commit_marker_stays_while_a_record_of_its_transaction_was_seen_and_goes_when_none_was() {
        let mut c = CleanedTxns::new(vec![]);
        assert!(!c.on_batch(9, true, 3));
        assert!(!c.on_marker(9, Some(true), 4));
        assert!(
            c.on_marker(9, Some(true), 8),
            "a second marker with no record before it"
        );
        assert!(
            !c.on_marker(9, None, 9),
            "a marker of a kind it does not know stays"
        );
    }
}
