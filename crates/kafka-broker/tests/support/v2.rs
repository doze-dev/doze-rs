//! Helpers of the KIP-890 tests (`transactions_v2*`).
#![allow(unreachable_pub, dead_code)]

use super::{Rig, Txn};

/// The producer `t` at `by` epochs later (or earlier).
pub fn later(t: Txn, by: i16) -> Txn {
    Txn {
        epoch: t.epoch + by,
        ..t
    }
}

/// The partitions `ps` of topic `t`, as DescribeTransactions lists them.
pub fn parts(t: &str, ps: &[i32]) -> Vec<(String, i32)> {
    ps.iter().map(|p| (t.to_owned(), *p)).collect()
}

impl Rig {
    /// (last stable offset, high watermark, aborted transactions) of a read-committed fetch of `t`-`p` from 0.
    pub async fn committed_view(&self, t: &str, p: i32) -> (i64, i64, usize) {
        let d = self.fetch_committed(t, p, 0).await;
        (
            d.last_stable_offset,
            d.high_watermark,
            d.aborted_transactions.as_ref().map_or(0, Vec::len),
        )
    }
}
