//! Acquiring a batch record by record (Kafka's `acquireSubsetBatchRecords`), and when that is how it must be: a record
//! that has been delivered often is delivered with fewer others (throttling), and one on its last delivery alone.

use std::cmp::max;

use super::{AcquiredRange, Taker};
use crate::share::partition::{Count, Limits, RecordState, SharePartition, Track, offset_of};

/// Kafka's `throttleRecordsDeliveryLimit`: half the delivery limit, rounded up, and at least 2.
fn throttle_limit(max_delivery: i32) -> i32 {
    max(2, (max_delivery + 1) / 2)
}

/// Kafka's `accumulateAcquiredRecords`: neighbouring offsets with the same delivery count, as one range.
fn accumulate(result: &mut Vec<AcquiredRange>, each: Vec<AcquiredRange>) {
    let mut each = each.into_iter();
    let Some(mut run) = each.next() else {
        return;
    };
    for r in each {
        if r.first == run.last + 1 && r.delivery_count == run.delivery_count {
            run.last = r.last;
        } else {
            result.push(run);
            run = r;
        }
    }
    result.push(run);
}

impl SharePartition {
    /// Kafka's `shouldThrottleRecordsDelivery`: the batch has an available record delivered at least half the limit.
    pub(super) fn should_throttle(
        &self,
        key: i64,
        req_first: i64,
        req_last: i64,
        limits: &Limits,
    ) -> bool {
        let limit = throttle_limit(limits.max_delivery);
        let Some(b) = self.cached.get(&key) else {
            return false;
        };
        match &b.track {
            Track::Whole(r) => r.state == RecordState::Available && i32::from(r.count) >= limit,
            Track::Each(_) => {
                b.records(key)
                    .filter(|(o, r)| {
                        *o >= req_first && *o <= req_last && r.state == RecordState::Available
                    })
                    .map(|(_, r)| i32::from(r.count))
                    .max()
                    .unwrap_or(0)
                    >= limit
            }
        }
    }

    /// Kafka's `acquireSubsetBatchRecords`: the available records of a batch kept per offset, from `req_first` to `req_last`.
    pub(super) fn acquire_subset(
        &mut self,
        t: &Taker<'_>,
        most: i64,
        (req_first, req_last): (i64, i64),
        key: i64,
        result: &mut Vec<AcquiredRange>,
    ) -> i64 {
        let (ask, max_delivery) = (t.ask, t.limits.max_delivery);
        let limit = throttle_limit(max_delivery);
        let Some(b) = self.cached.get_mut(&key) else {
            return 0;
        };
        let Track::Each(records) = &mut b.track else {
            return 0;
        };
        let size = offset_of(records.len());
        let mut acquired = 0;
        let mut most_while_throttled: Option<i64> = None;
        let mut taken = Vec::new();
        for (i, r) in records.iter_mut().enumerate() {
            let offset = key + offset_of(i);
            if offset < req_first {
                continue;
            }
            if offset > req_last {
                break;
            }
            if r.state != RecordState::Available {
                continue;
            }
            let count = i32::from(r.count);
            // On its last delivery a record goes alone: one more with others would spend theirs if it is the bad one.
            if max_delivery > 2 && count == max_delivery - 1 && acquired > 0 {
                break;
            }
            if count >= limit && most_while_throttled.is_none() {
                let shift = u32::try_from(count - limit + 1).unwrap_or(u32::MAX);
                most_while_throttled = Some(max(1, size.checked_shr(shift).unwrap_or(0)));
            }
            let Some(mut next) = r.moved(
                RecordState::Acquired,
                Count::Increase,
                max_delivery,
                Some(&ask.member),
            ) else {
                continue;
            };
            next.until = Some(t.until);
            *r = next;
            taken.push(AcquiredRange {
                first: offset,
                last: offset,
                delivery_count: r.count,
            });
            acquired += 1;
            if i32::from(r.count) == max_delivery && max_delivery > 2 {
                break;
            }
            if ask.record_limit && acquired == most {
                break;
            }
            if most_while_throttled == Some(acquired) {
                break;
            }
        }
        accumulate(result, taken);
        acquired
    }
}
