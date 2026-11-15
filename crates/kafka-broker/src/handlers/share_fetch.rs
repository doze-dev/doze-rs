//! ShareFetch (KIP-932): records acquired for one member of a share group, with the acknowledgements of what it had before
//! carried on the same request. Kafka 4.4.0's order (`KafkaApis.handleShareFetchRequest`): share groups on, a group id,
//! READ on the group, a member id, a renew-only request's zeros; then the share session (an error there is the whole
//! answer); then the acknowledgements; then the fetch, which waits up to MaxWaitMs for something to acquire.
//!
//! What a response says of each partition: an error, or the batches read and the ranges of them the member now holds (with
//! how often each has been delivered). An incremental fetch reports only partitions with something to say; every partition
//! acknowledged is reported, with how its acknowledgement went.

mod read;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use kafka_auth::authz::Operation;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::StrBytes;
use kafka_wire::Uuid;
use kafka_wire::ids::{GroupId, MemberId, Offset, PartitionId, TopicId};
use kafka_wire::messages::share_fetch_response::{AcquiredRecords, PartitionData, ShareFetchableTopicResponse};
use kafka_wire::messages::{ShareFetchRequest, ShareFetchResponse};
use tokio::time::{Instant, timeout_at};
use tokio_stream::wrappers::WatchStream;
use tokio_stream::{StreamExt, StreamMap};

use super::share_acks::{Acked, Asked};
use super::share_group_heartbeat::member_id_valid;
use crate::partitions::Partition;
use crate::share::partition::{AckBatch, AcquireAsk};
use crate::share::sessions::{Context, Part};
use crate::share::{Held, limits_of};
use crate::{Broker, Caller, Listener, Outcome, clock};

/// The longest a ShareFetch waits for something to acquire, whatever the client asked (as Fetch: an answer with nothing is
/// always correct, and a wait holds a connection that cannot see its client leave).
const MAX_WAIT: Duration = Duration::from_secs(30);
/// The most one response holds whatever the client asks (Kafka's `fetch.max.bytes`).
const MOST_PER_REQUEST: i64 = 50 << 20;

/// One partition's part of the answer.
struct Found {
    error: Option<E>,
    records: Bytes,
    acquired: Vec<AcquiredRecords>,
}

impl Found {
    fn error(e: E) -> Self {
        Self {
            error: Some(e),
            records: Bytes::new(),
            acquired: Vec::new(),
        }
    }

    fn nothing() -> Self {
        Self {
            error: None,
            records: Bytes::new(),
            acquired: Vec::new(),
        }
    }
}

/// A partition the fetch may read.
struct Place<F: Fs> {
    part: Part,
    held: Arc<Held>,
    log: Arc<Partition<F>>,
}

fn top_level(error: E) -> Outcome {
    let mut resp = ShareFetchResponse::default();
    resp.error_code = error.code();
    Outcome::Respond(kafka_wire::api::Response::ShareFetch(resp))
}

/// The partitions a request names and those it forgets.
fn named(req: &ShareFetchRequest) -> (Vec<Part>, Vec<Part>) {
    let asked = req
        .topics
        .iter()
        .flat_map(|t| t.partitions.iter().map(move |p| (t.topic_id, p.partition_index)))
        .collect();
    let forgotten = req
        .forgotten_topics_data
        .iter()
        .flat_map(|t| t.partitions.iter().map(move |p| (t.topic_id, *p)))
        .collect();
    (asked, forgotten)
}

fn acks_of(req: &ShareFetchRequest) -> Vec<Asked> {
    req.topics
        .iter()
        .flat_map(|t| {
            t.partitions.iter().map(move |p| Asked {
                topic: t.topic_id,
                partition: p.partition_index,
                batches: p
                    .acknowledgement_batches
                    .iter()
                    .map(|b| AckBatch {
                        first: b.first_offset,
                        last: b.last_offset,
                        types: b.acknowledge_types.clone(),
                    })
                    .collect(),
            })
        })
        .collect()
}

impl<F: Fs> Broker<F> {
    pub(crate) async fn share_fetch(
        &self,
        version: i16,
        listener: &Listener,
        caller: &Caller,
        req: ShareFetchRequest,
    ) -> Outcome {
        let Some(group_name) = req.group_id.as_ref().map(|g| g.0.as_str().to_owned()) else {
            return top_level(E::InvalidRequest);
        };
        if !self.guard(caller).group(Operation::Read, &group_name) {
            return top_level(E::GroupAuthorizationFailed);
        }
        let member_text = req.member_id.as_ref().map(|m| m.as_str().to_owned());
        if !member_id_valid(member_text.as_deref()) {
            return top_level(E::InvalidRequest);
        }
        let renew_only = version >= 2 && req.is_renew_ack;
        if renew_only && (req.max_bytes != 0 || req.min_bytes != 0 || req.max_records != 0 || req.max_wait_ms != 0) {
            return top_level(E::InvalidRequest);
        }
        let (Ok(group), Some(member)) = (
            GroupId::parse(&group_name),
            member_text.as_deref().and_then(MemberId::new),
        ) else {
            return top_level(E::InvalidRequest);
        };
        let acks = acks_of(&req);
        let has_acks = acks.iter().any(|a| !a.batches.is_empty());
        let (asked, forgotten) = named(&req);
        let session = (group.clone(), member.clone());
        let context = self.shares.sessions().fetch(
            session.clone(),
            req.share_session_epoch,
            (&asked, &forgotten),
            has_acks,
            listener.connection,
        );
        let context = match context {
            Ok(c) => c,
            Err(E::ShareSessionLimitReached) => {
                // Kafka holds this answer for the request's wait (`IdleShareFetchTimerTask`), so a client retrying does
                // not spin.
                let wait = Duration::from_millis(u64::try_from(req.max_wait_ms).unwrap_or(0)).min(MAX_WAIT);
                tokio::time::sleep(wait).await;
                return top_level(E::ShareSessionLimitReached);
            }
            Err(e) => return top_level(e),
        };
        let acked = if has_acks {
            self.share_acknowledge_all(caller, (&group, member.as_str()), &acks, (version, req.is_renew_ack))
        } else {
            Vec::new()
        };
        let settings = self.share_settings(&group);
        let fetching: Vec<Part> = match &context {
            Context::Full(parts) | Context::Incremental(parts) => parts.clone(),
            Context::Final => Vec::new(),
        };
        let fetches = !renew_only && req.max_records > 0 && req.max_bytes > 0 && !fetching.is_empty();
        if fetches && !(0..=1).contains(&req.share_acquire_mode) {
            return top_level(E::UnknownServerError);
        }
        let found = if fetches {
            let ask = AcquireAsk {
                member: Arc::from(member.as_str()),
                record_limit: req.share_acquire_mode == 1,
                batch_size: req.batch_size,
                max_records: i64::from(req.max_records),
                read_committed: settings.read_committed,
            };
            self.share_fetch_wait(caller, &group, &ask, &fetching, &req).await
        } else {
            BTreeMap::new()
        };
        let mut resp = ShareFetchResponse::default();
        resp.acquisition_lock_timeout_ms = i32::try_from(settings.lock_ms).unwrap_or(i32::MAX);
        let reported: Vec<Part> = match &context {
            Context::Incremental(_) => {
                let mut sessions = self.shares.sessions();
                if sessions.still_at(&session, req.share_session_epoch) {
                    let results: Vec<(Part, bool, bool)> = found
                        .iter()
                        .map(|(p, f)| (*p, !f.acquired.is_empty(), f.error.is_some()))
                        .collect();
                    sessions.report(&session, &results)
                } else {
                    // Another request moved the session meanwhile (Kafka's check): the fetch is not reported.
                    resp.error_code = E::InvalidShareSessionEpoch.code();
                    Vec::new()
                }
            }
            Context::Full(_) | Context::Final => found.keys().copied().collect(),
        };
        let mut found = found;
        let mut topics: Vec<ShareFetchableTopicResponse> = Vec::new();
        for part in reported {
            let Some(f) = found.remove(&part) else {
                continue;
            };
            let data = topic_entry(&mut topics, part.0);
            let mut d = PartitionData::default();
            d.partition_index = part.1;
            d.error_code = f.error.map_or(0, |e| e.code());
            d.records = Some(f.records);
            d.acquired_records = f.acquired;
            data.partitions.push(d);
        }
        merge_acks(&mut topics, acked);
        resp.responses = topics;
        if matches!(context, Context::Final) {
            self.close_share_session(&group, &member);
        }
        Outcome::Respond(kafka_wire::api::Response::ShareFetch(resp))
    }

    /// Closes a member's share session and lets go of what it held there (Kafka's `releaseSession`).
    pub(crate) fn close_share_session(&self, group: &GroupId, member: &MemberId) {
        let key = (group.clone(), member.clone());
        let parts = {
            let mut sessions = self.shares.sessions();
            let parts = sessions.parts(&key);
            sessions.close(&key);
            parts
        };
        self.release_share_member(group, member.as_str(), &parts);
    }

    /// Acquires for the member until something is acquired, an error is found, or the wait is over.
    async fn share_fetch_wait(
        &self,
        caller: &Caller,
        group: &GroupId,
        ask: &AcquireAsk,
        parts: &[Part],
        req: &ShareFetchRequest,
    ) -> BTreeMap<Part, Found> {
        let waits = Duration::from_millis(u64::try_from(req.max_wait_ms).unwrap_or(0)).min(MAX_WAIT);
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
            let failed = found.values().any(|f| f.error.is_some()) || got.values().any(|f| f.error.is_some());
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
                let got = crate::blocking::run(|| self.share_round(group, ask, &places, req.max_bytes));
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
            let Some(topic) = TopicId::from_uuid(part.0).and_then(|id| self.meta.topic_by_id(id)) else {
                found.insert(*part, Found::error(E::UnknownTopicId));
                continue;
            };
            if !guard.topic(Operation::Read, topic.name.as_str()) {
                found.insert(*part, Found::error(E::TopicAuthorizationFailed));
                continue;
            }
            let Some(partition) = PartitionId::new(part.1).filter(|p| p.get() < topic.partitions.get()) else {
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
                Ok(log) => places.push(Place { part: *part, held, log }),
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
                read::Read::Data { bytes, spans, aborted } => {
                    let only: Vec<_> = spans.iter().map(|(s, _)| *s).collect();
                    let this = AcquireAsk {
                        max_records: records_left,
                        ..ask.clone()
                    };
                    let got = part.acquire(&this, from, &only, &aborted, now, &limits);
                    if got.ranges.is_empty() {
                        out.insert(place.part, Found::nothing());
                        continue;
                    }
                    if let Some(at) = part.next_deadline() {
                        self.shares.alarm.set(at);
                    }
                    records_left -= got.count;
                    let records = read::slice(&bytes, &spans, &got.ranges);
                    budget = budget.saturating_sub(i64::try_from(records.len()).unwrap_or(i64::MAX));
                    out.insert(
                        place.part,
                        Found {
                            error: None,
                            records,
                            acquired: got
                                .ranges
                                .iter()
                                .map(|r| {
                                    let mut a = AcquiredRecords::default();
                                    a.first_offset = r.first;
                                    a.last_offset = r.last;
                                    a.delivery_count = r.delivery_count;
                                    a
                                })
                                .collect(),
                        },
                    );
                }
            }
        }
        out
    }
}

/// The response's entry for a topic, made if there is none.
fn topic_entry(topics: &mut Vec<ShareFetchableTopicResponse>, topic: Uuid) -> &mut ShareFetchableTopicResponse {
    let i = match topics.iter().position(|t| t.topic_id == topic) {
        Some(i) => i,
        None => {
            let mut t = ShareFetchableTopicResponse::default();
            t.topic_id = topic;
            topics.push(t);
            topics.len() - 1
        }
    };
    &mut topics[i]
}

/// Each acknowledged partition's result: on its entry if the fetch reported it, else on an entry of its own with no
/// records (Kafka adds them after the session pruned the fetch).
fn merge_acks(topics: &mut Vec<ShareFetchableTopicResponse>, acked: Vec<Acked>) {
    for a in acked {
        let t = topic_entry(topics, a.topic);
        let i = match t.partitions.iter().position(|p| p.partition_index == a.partition) {
            Some(i) => i,
            None => {
                let mut d = PartitionData::default();
                d.partition_index = a.partition;
                d.records = Some(Bytes::new());
                t.partitions.push(d);
                t.partitions.len() - 1
            }
        };
        let d = &mut t.partitions[i];
        d.acknowledge_error_code = a.error.map_or(0, |e| e.code());
        d.acknowledge_error_message = a.message.map(StrBytes::from_string);
    }
}
