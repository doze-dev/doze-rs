//! `ParallelizationFactor` for the Kinesis and DynamoDB stream pollers: up to that many batches of one shard are processed at once,
//! and the records of one partition key stay in order (AWS Lambda Developer Guide, "Parallelization factor": Lambda splits a shard's
//! records into lanes by partition key; the key of a DynamoDB record is the item's key).
//!
//! The poller reads up to `BatchSize × factor` records; they are dealt into `factor` lanes by a hash of their key, each lane is
//! delivered in batches of `BatchSize` in its own task with the shared retry, bisect and failure semantics ([`deliver_batches`]), and
//! the shard's position moves past the whole read only when every lane is over, so a restart reads again what was not finished
//! (at-least-once, as on AWS). A factor of 1 is the plain path: one batch at a time.

use super::Run;
use super::streams::{Delivered, Rec, deliver_batches};

/// The factor a mapping asks for (1 to 10, the model's range).
pub(super) fn factor(run: &Run) -> usize {
    usize::try_from(run.int("ParallelizationFactor", 1).clamp(1, 10)).unwrap_or(1)
}

/// How many records to read from a shard at once: a batch for each lane.
pub(super) fn read_size(run: &Run, batch: i64) -> i64 {
    (batch * i64::try_from(factor(run)).unwrap_or(1)).clamp(1, 10_000)
}

/// FNV-1a: a stable hash, so a key keeps its lane across reads and restarts.
fn lane_of(key: &str, lanes: usize) -> usize {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    usize::try_from(h % u64::try_from(lanes.max(1)).unwrap_or(1)).unwrap_or(0)
}

/// The records dealt into `lanes` lanes by key, each in the order it was read.
pub(super) fn split(records: Vec<Rec>, lanes: usize) -> Vec<Vec<Rec>> {
    let mut out: Vec<Vec<Rec>> = (0..lanes.max(1)).map(|_| Vec::new()).collect();
    for r in records {
        let lane = lane_of(&r.key, lanes);
        if let Some(l) = out.get_mut(lane) {
            l.push(r);
        }
    }
    out
}

/// Delivers what was read from one shard: in one lane when the factor is 1, else in parallel lanes, saving the position after all.
pub(super) async fn deliver(run: &Run, kind: &str, shard: &str, records: Vec<Rec>) -> Delivered {
    let lanes = factor(run);
    if lanes == 1 {
        return deliver_batches(run, kind, shard, records, true).await;
    }
    let size = usize::try_from(run.int("BatchSize", 100).clamp(1, 10_000)).unwrap_or(100);
    let Some(last) = records.last().map(|r| r.pos.clone()) else {
        return Delivered::Done;
    };
    let mut set = tokio::task::JoinSet::new();
    for lane in split(records, lanes).into_iter().filter(|l| !l.is_empty()) {
        let (run, kind, shard) = (run.clone(), kind.to_owned(), shard.to_owned());
        set.spawn(async move {
            for chunk in lane.chunks(size) {
                if matches!(
                    deliver_batches(&run, &kind, &shard, chunk.to_vec(), false).await,
                    Delivered::Stopped
                ) {
                    return Delivered::Stopped;
                }
            }
            Delivered::Done
        });
    }
    let mut stopped = false;
    while let Some(done) = set.join_next().await {
        stopped |= !matches!(done, Ok(Delivered::Done));
    }
    if stopped {
        return Delivered::Stopped;
    }
    run.save_position(shard, &last).await;
    Delivered::Done
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(key: &str, seq: u128) -> Rec {
        Rec {
            pos: seq.to_string(),
            seq,
            key: key.to_owned(),
            doc: serde_json::Value::Null,
            filter: serde_json::Value::Null,
            arrived_ms: 0,
        }
    }

    #[test]
    fn a_key_keeps_its_lane_and_its_order() {
        let records: Vec<Rec> = (0..40).map(|i| rec(&format!("k{}", i % 7), i)).collect();
        let lanes = split(records, 4);
        assert_eq!(lanes.len(), 4);
        assert!(
            lanes.iter().filter(|l| !l.is_empty()).count() > 1,
            "keys spread"
        );
        for lane in &lanes {
            assert!(lane.windows(2).all(|w| w[0].seq < w[1].seq), "in order");
        }
        for k in 0..7 {
            let key = format!("k{k}");
            let holding = lanes
                .iter()
                .filter(|l| l.iter().any(|r| r.key == key))
                .count();
            assert_eq!(holding, 1, "{key} is in one lane");
        }
        assert_eq!(split(vec![rec("a", 1)], 1)[0].len(), 1);
    }
}
