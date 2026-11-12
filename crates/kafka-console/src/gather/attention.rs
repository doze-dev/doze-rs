//! The one condition Home draws as needing someone: a transaction close to its timeout.
//!
//! "Close" is two thirds of the timeout gone, which the board's own example (41 s of 60 s) meets. The coordinator aborts
//! a transaction at its timeout and fences the producer, so this is the last stretch in which a person can act.
//! It is judged on what the protocol says (ListTransactions, DescribeTransactions), and the held-back figure on what a
//! `read_committed` consumer would be told (ListOffsets).

use kafka_wire::api::Response;

use crate::backend::{Backend, requests};
use crate::format::clock;
use crate::model::{Attention, HeldBack};

/// Whether `elapsed_ms` of a `timeout_ms` is close to the end (two thirds or more).
#[must_use]
pub fn near_timeout(elapsed_ms: i64, timeout_ms: i64) -> bool {
    timeout_ms > 0 && elapsed_ms.saturating_mul(3) >= timeout_ms.saturating_mul(2)
}

/// The open transaction nearest to its timeout among those that are close, and how many are close.
pub async fn nearest(b: &Backend, now_ms: i64) -> Option<Attention> {
    let Ok(Response::ListTransactions(list)) = b.call(requests::list_ongoing_transactions()).await
    else {
        return None;
    };
    let ids: Vec<String> = list
        .transaction_states
        .iter()
        .map(|t| t.transactional_id.0.to_string())
        .collect();
    if ids.is_empty() {
        return None;
    }
    let Ok(Response::DescribeTransactions(d)) = b.call(requests::describe_transactions(&ids)).await
    else {
        return None;
    };
    let mut close: Vec<_> = d
        .transaction_states
        .into_iter()
        .filter(|t| t.error_code == 0 && t.transaction_state.as_str() == "Ongoing")
        .filter(|t| {
            near_timeout(
                now_ms - t.transaction_start_time_ms,
                i64::from(t.transaction_timeout_ms),
            )
        })
        .collect();
    // The nearest to its end is the one with the least time left.
    close.sort_by_key(|t| {
        i64::from(t.transaction_timeout_ms) - (now_ms - t.transaction_start_time_ms)
    });
    let count = close.len();
    let first = close.into_iter().next()?;
    let (topic, partition) = first
        .topics
        .iter()
        .find_map(|t| t.partitions.first().map(|p| (t.topic.0.to_string(), *p)))?;
    Some(Attention {
        count,
        transactional_id: first.transactional_id.0.to_string(),
        producer_id: first.producer_id.0,
        producer_epoch: first.producer_epoch,
        partition: format!("{topic}-{partition}"),
        held_back: held_back(b, &topic, partition).await,
        topic,
        since: format!("{} UTC", clock(first.transaction_start_time_ms)),
        timeout_secs: i64::from(first.transaction_timeout_ms) / 1000,
        elapsed_secs: ((now_ms - first.transaction_start_time_ms) / 1000).max(0),
    })
}

/// The last stable offset and the high watermark of a partition: what `read_committed` and `read_uncommitted` end at.
async fn held_back(b: &Backend, topic: &str, partition: i32) -> Option<HeldBack> {
    let end = |isolation| async move {
        let Ok(Response::ListOffsets(r)) = b
            .call(requests::latest_offset(topic, partition, isolation))
            .await
        else {
            return None;
        };
        let p = r.topics.first()?.partitions.first()?;
        (p.error_code == 0).then_some(p.offset)
    };
    Some(HeldBack {
        last_stable: end(1).await?,
        high_watermark: end(0).await?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_thirds_of_the_timeout_is_close() {
        // The board's example: 41 s of 60 s.
        assert!(near_timeout(41_000, 60_000));
        assert!(near_timeout(40_000, 60_000));
        assert!(!near_timeout(39_999, 60_000));
        assert!(near_timeout(60_000, 60_000));
        assert!(near_timeout(90_000, 60_000));
        assert!(!near_timeout(0, 60_000));
    }

    #[test]
    fn a_timeout_of_nothing_is_never_close_and_nothing_overflows() {
        assert!(!near_timeout(1000, 0));
        assert!(!near_timeout(1000, -5));
        assert!(near_timeout(i64::MAX, 1));
        assert!(!near_timeout(i64::MIN, i64::MAX));
    }
}
