//! ShareFetch (KIP-932): records acquired for one member of a share group, with the acknowledgements of what it had before
//! carried on the same request. Kafka 4.4.0's order (`KafkaApis.handleShareFetchRequest`): share groups on, a group id,
//! READ on the group, a member id, a renew-only request's zeros; then the share session (an error there is the whole
//! answer); then the acknowledgements; then the fetch, which waits up to MaxWaitMs for something to acquire.
//!
//! What a response says of each partition: an error, or the batches read and the ranges of them the member now holds (with
//! how often each has been delivered). An incremental fetch reports only partitions with something to say; every partition
//! acknowledged is reported, with how its acknowledgement went.

mod read;
mod round;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use kafka_auth::authz::Operation;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::StrBytes;
use kafka_wire::Uuid;
use kafka_wire::ids::{GroupId, MemberId};
use kafka_wire::messages::share_fetch_response::{
    AcquiredRecords, PartitionData, ShareFetchableTopicResponse,
};
use kafka_wire::messages::{ShareFetchRequest, ShareFetchResponse};

use super::share_acks::{Acked, Asked};
use super::share_group_heartbeat::member_id_valid;
use crate::share::partition::{AckBatch, AcquireAsk, AcquiredRange};
use crate::share::sessions::{Context, Part};
use crate::{Broker, Caller, Listener, Outcome};

/// The longest a ShareFetch waits for something to acquire, whatever the client asked (as Fetch: an answer with nothing is
/// always correct, and a wait holds a connection that cannot see its client leave).
const MAX_WAIT: Duration = Duration::from_secs(30);
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

    fn acquired(records: Bytes, ranges: &[AcquiredRange]) -> Self {
        Self {
            error: None,
            records,
            acquired: ranges
                .iter()
                .map(|r| {
                    let mut a = AcquiredRecords::default();
                    a.first_offset = r.first;
                    a.last_offset = r.last;
                    a.delivery_count = r.delivery_count;
                    a
                })
                .collect(),
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
        .flat_map(|t| {
            t.partitions
                .iter()
                .map(move |p| (t.topic_id, p.partition_index))
        })
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
        let renew_only = version >= 2 && req.is_renew_ack;
        let (group, member) = match self.share_fetch_who(caller, &req, renew_only) {
            Ok(who) => who,
            Err(e) => return top_level(e),
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
                let wait = Duration::from_millis(u64::try_from(req.max_wait_ms).unwrap_or(0))
                    .min(MAX_WAIT);
                tokio::time::sleep(wait).await;
                return top_level(E::ShareSessionLimitReached);
            }
            Err(e) => return top_level(e),
        };
        let acked = if has_acks {
            self.share_acknowledge_all(
                caller,
                (&group, member.as_str()),
                &acks,
                (version, req.is_renew_ack),
            )
        } else {
            Vec::new()
        };
        let settings = self.share_settings(&group);
        let fetching: Vec<Part> = match &context {
            Context::Full(parts) | Context::Incremental(parts) => parts.clone(),
            Context::Final => Vec::new(),
        };
        let fetches =
            !renew_only && req.max_records > 0 && req.max_bytes > 0 && !fetching.is_empty();
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
            self.share_fetch_wait(caller, &group, &ask, &fetching, &req)
                .await
        } else {
            BTreeMap::new()
        };
        let mut resp = ShareFetchResponse::default();
        resp.acquisition_lock_timeout_ms = i32::try_from(settings.lock_ms).unwrap_or(i32::MAX);
        (resp.responses, resp.error_code) =
            self.share_fetch_report(&session, &req, &context, found, acked);
        if matches!(context, Context::Final) {
            self.close_share_session(&group, &member);
        }
        Outcome::Respond(kafka_wire::api::Response::ShareFetch(resp))
    }

    /// Who fetches, after Kafka's request-wide checks: the group named and readable, a valid member id, and a renewal
    /// with nothing to fetch.
    fn share_fetch_who(
        &self,
        caller: &Caller,
        req: &ShareFetchRequest,
        renew_only: bool,
    ) -> Result<(GroupId, MemberId), E> {
        let group_name = req
            .group_id
            .as_ref()
            .map(|g| g.0.as_str())
            .ok_or(E::InvalidRequest)?;
        if !self.guard(caller).group(Operation::Read, group_name) {
            return Err(E::GroupAuthorizationFailed);
        }
        let member_text = req.member_id.as_ref().map(StrBytes::as_str);
        if !member_id_valid(member_text) {
            return Err(E::InvalidRequest);
        }
        if renew_only
            && (req.max_bytes != 0
                || req.min_bytes != 0
                || req.max_records != 0
                || req.max_wait_ms != 0)
        {
            return Err(E::InvalidRequest);
        }
        match (
            GroupId::parse(group_name),
            member_text.and_then(MemberId::new),
        ) {
            (Ok(group), Some(member)) => Ok((group, member)),
            _ => Err(E::InvalidRequest),
        }
    }

    /// What the response tells of each partition: all that was looked at, or (an incremental fetch) what the session says
    /// is worth telling; then each acknowledgement's result. With the top-level error: an incremental fetch whose session
    /// another request moved meanwhile is not reported (Kafka's check).
    fn share_fetch_report(
        &self,
        session: &(GroupId, MemberId),
        req: &ShareFetchRequest,
        context: &Context,
        mut found: BTreeMap<Part, Found>,
        acked: Vec<Acked>,
    ) -> (Vec<ShareFetchableTopicResponse>, i16) {
        let mut error_code = 0;
        let reported: Vec<Part> = match context {
            Context::Incremental(_) => {
                let mut sessions = self.shares.sessions();
                if sessions.still_at(session, req.share_session_epoch) {
                    let results: Vec<(Part, bool, bool)> = found
                        .iter()
                        .map(|(p, f)| (*p, !f.acquired.is_empty(), f.error.is_some()))
                        .collect();
                    sessions.report(session, &results)
                } else {
                    // Another request moved the session meanwhile (Kafka's check): the fetch is not reported.
                    error_code = E::InvalidShareSessionEpoch.code();
                    Vec::new()
                }
            }
            Context::Full(_) | Context::Final => found.keys().copied().collect(),
        };
        let mut topics: Vec<ShareFetchableTopicResponse> = Vec::new();
        for part in reported {
            let Some(f) = found.remove(&part) else {
                continue;
            };
            let mut d = PartitionData::default();
            d.partition_index = part.1;
            d.error_code = f.error.map_or(0, |e| e.code());
            d.records = Some(f.records);
            d.acquired_records = f.acquired;
            push_partition(&mut topics, part.0, d);
        }
        merge_acks(&mut topics, acked);
        (topics, error_code)
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
}

/// A partition's answer, on its topic's entry (made the first time the topic is answered).
fn push_partition(topics: &mut Vec<ShareFetchableTopicResponse>, topic: Uuid, d: PartitionData) {
    match topics.iter_mut().find(|t| t.topic_id == topic) {
        Some(t) => t.partitions.push(d),
        None => {
            let mut t = ShareFetchableTopicResponse::default();
            t.topic_id = topic;
            t.partitions.push(d);
            topics.push(t);
        }
    }
}

/// Each acknowledged partition's result: on its entry if the fetch reported it, else on an entry of its own with no
/// records (Kafka adds them after the session pruned the fetch).
fn merge_acks(topics: &mut Vec<ShareFetchableTopicResponse>, acked: Vec<Acked>) {
    for a in acked {
        let code = a.error.map_or(0, |e| e.code());
        let message = a.message.map(StrBytes::from_string);
        let reported = topics
            .iter_mut()
            .find(|t| t.topic_id == a.topic)
            .and_then(|t| {
                t.partitions
                    .iter_mut()
                    .find(|p| p.partition_index == a.partition)
            });
        if let Some(d) = reported {
            d.acknowledge_error_code = code;
            d.acknowledge_error_message = message;
            continue;
        }
        let mut d = PartitionData::default();
        d.partition_index = a.partition;
        d.records = Some(Bytes::new());
        d.acknowledge_error_code = code;
        d.acknowledge_error_message = message;
        push_partition(topics, a.topic, d);
    }
}
