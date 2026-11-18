//! The acquiring side of a ShareFetch: the partitions it may read, one round of reading and acquiring over them, and
//! the wait until a round acquires something.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use kafka_auth::authz::Operation;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::ids::{GroupId, Offset, PartitionId, TopicId};
use kafka_wire::messages::ShareFetchRequest;
use tokio::time::{Instant, timeout_at};
use tokio_stream::wrappers::WatchStream;
use tokio_stream::{StreamExt, StreamMap};

use super::{Found, MAX_WAIT, read};
use crate::partitions::Partition;
use crate::share::partition::{AcquireAsk, Fetched};
use crate::share::sessions::Part;
use crate::share::{Held, limits_of};
use crate::{Broker, Caller, clock};

/// The most one response holds whatever the client asks (Kafka's `fetch.max.bytes`).
const MOST_PER_REQUEST: i64 = 50 << 20;

/// A partition the fetch may read.
struct Place<F: Fs> {
    part: Part,
    held: Arc<Held>,
    log: Arc<Partition<F>>,
}

impl<F: Fs> Broker<F> {
    /// Acquires for the member until something is acquired, an error is found, or the wait is over.
    pub(super) async fn share_fetch_wait(
        &self,
        caller: &Caller,
        group: &GroupId,
        ask: &AcquireAsk,
        parts: &[Part],
        req: &ShareFetchRequest,
    ) -> BTreeMap<Part, Found> {
        let waits =
            Duration::from_millis(u64::try_from(req.max_wait_ms).unwrap_or(0)).min(MAX_WAIT);
        let deadline = Instant::now() + waits;
        // The order is turned by the session's epoch, so a partition first in the request is not always read first.
        let mut order: Vec<Part> = parts.to_vec();
        if req.share_session_epoch >= 1 && !order.is_empty() {
            let n = usize::try_from(req.share_session_epoch).unwrap_or(0) % order.len();
            order.rotate_left(n);
        }
        loop {
            // Watched from before the read: a change after it wakes the wait, one before it was read.
            let mut changes = WatchStream::from_changes(self.shares.watch());
            let mut topics = WatchStream::from_changes(self.topics_changed.subscribe());
            let (mut found, places) = self.share_places(caller, group, &order).await;
            let mut ends: StreamMap<usize, WatchStream<Offset>> = places
                .iter()
                .enumerate()
                .map(|(i, p)| (i, WatchStream::from_changes(p.log.reader().watch_end())))
                .collect();
            let got = crate::blocking::run(|| self.share_round(group, ask, &places, req.max_bytes));
            let acquired = got.values().any(|f| !f.acquired.is_empty());
            let failed = found.values().any(|f| f.error.is_some())
                || got.values().any(|f| f.error.is_some());
            found.extend(got);
            if acquired || failed || waits.is_zero() || Instant::now() >= deadline {
                return found;
            }
            let woke = async {
                tokio::select! {
                    Some(_) = changes.next() => {}
                    Some(_) = topics.next() => {}
                    Some(_) = ends.next(), if !ends.is_empty() => {}
                    else => std::future::pending().await,
                }
            };
            if timeout_at(deadline, woke).await.is_err() {
                // One last look, as Kafka makes at the end of the wait.
                let (mut found, places) = self.share_places(caller, group, &order).await;
                let got =
                    crate::blocking::run(|| self.share_round(group, ask, &places, req.max_bytes));
                found.extend(got);
                return found;
            }
        }
    }

    /// The partitions the fetch may read, and the answers of those it may not.
    async fn share_places(
        &self,
        caller: &Caller,
        group: &GroupId,
        parts: &[Part],
    ) -> (BTreeMap<Part, Found>, Vec<Place<F>>) {
        let guard = self.guard(caller);
        let mut found = BTreeMap::new();
        let mut places = Vec::new();
        for part in parts {
            let Some(topic) = TopicId::from_uuid(part.0).and_then(|id| self.meta.topic_by_id(id))
            else {
                found.insert(*part, Found::error(E::UnknownTopicId));
                continue;
            };
            if !guard.topic(Operation::Read, topic.name.as_str()) {
                found.insert(*part, Found::error(E::TopicAuthorizationFailed));
                continue;
            }
            let Some(partition) =
                PartitionId::new(part.1).filter(|p| p.get() < topic.partitions.get())
            else {
                found.insert(*part, Found::error(E::UnknownTopicOrPartition));
                continue;
            };
            let held = match self.share_partition(group, &topic, partition).await {
                Ok(h) => h,
                Err(e) => {
                    found.insert(*part, Found::error(e.code()));
                    continue;
                }
            };
            match self.partitions.get(&topic, partition).await {
                Ok(log) => places.push(Place {
                    part: *part,
                    held,
                    log,
                }),
                Err(e) => {
                    tracing::error!(error = %e, topic = %topic.name, partition = part.1, "a share fetch could not open a partition");
                    found.insert(*part, Found::error(E::KafkaStorageError));
                }
            }
        }
        (found, places)
    }

    /// One pass: each partition read from its next fetch offset and what can be acquired of it acquired, the record limit
    /// shared by them all. A partition that can acquire nothing now (at its in-flight limit) is not in the answer, as in
    /// Kafka.
    fn share_round(
        &self,
        group: &GroupId,
        ask: &AcquireAsk,
        places: &[Place<F>],
        max_bytes: i32,
    ) -> BTreeMap<Part, Found> {
        let limits = limits_of(&self.share_settings(group));
        let now = clock::now().get();
        let mut budget = i64::from(max_bytes).clamp(1, MOST_PER_REQUEST);
        let mut records_left = ask.max_records;
        let mut out = BTreeMap::new();
        for place in places {
            if records_left <= 0 {
                break;
            }
            let mut part = place.held.lock();
            if !part.can_acquire(&limits) {
                continue;
            }
            let from = part.next_fetch_offset();
            let limit = usize::try_from(budget.max(1)).unwrap_or(1);
            match read::read(&place.log, from, limit, ask.read_committed) {
                read::Read::Failed => {
                    out.insert(place.part, Found::error(E::KafkaStorageError));
                }
                read::Read::Gone { log_start } => {
                    // Kafka moves the start up to the log's and answers this round with nothing.
                    let before = part.start;
                    part.log_start_moved(log_start);
                    if part.start != before {
                        self.shares.notify();
                    }
                    out.insert(place.part, Found::nothing());
                }
                read::Read::Data {
                    bytes,
                    spans,
                    aborted,
                } => {
                    let only: Vec<_> = spans.iter().map(|(s, _)| *s).collect();
                    let this = AcquireAsk {
                        max_records: records_left,
                        ..ask.clone()
                    };
                    let read = Fetched {
                        from,
                        spans: &only,
                        aborted: &aborted,
                    };
                    let got = part.acquire(&this, &read, now, &limits);
                    if got.ranges.is_empty() {
                        out.insert(place.part, Found::nothing());
                        continue;
                    }
                    if let Some(at) = part.next_deadline() {
                        self.shares.alarm.set(at);
                    }
                    records_left -= got.count;
                    let records = read::slice(&bytes, &spans, &got.ranges);
                    budget =
                        budget.saturating_sub(i64::try_from(records.len()).unwrap_or(i64::MAX));
                    out.insert(place.part, Found::acquired(records, &got.ranges));
                }
            }
        }
        out
    }
}
