//! Fetch.

mod round;
mod wait;

use std::sync::Arc;
use std::time::Duration;

use kafka_auth::authz::Operation;
use kafka_meta::Topic;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::api::Response;
use kafka_wire::ids::{PartitionId, TopicId, TopicName};
use kafka_wire::messages::fetch_request::FetchTopic;
use kafka_wire::messages::{FetchRequest, FetchResponse};
use tokio::time::{Instant, timeout_at};

use crate::partitions::Partition;
use crate::{Broker, Caller, Outcome};
use round::Budget;

/// The longest a Fetch waits for data, whatever the client asked for. A wait is inside a connection that cannot
/// notice its client going away until it writes the answer, so an unbounded one would hold a socket for as long as
/// the client's timeout, and answering early with nothing is always correct: the client fetches again.
const MAX_WAIT: Duration = Duration::from_secs(30);

/// The broker a Fetch comes from, or -1 for a consumer (`FetchRequest.replicaId`): the top-level `ReplicaId` up to v14, and
/// from v15 (KIP-903) the `ReplicaState` tagged field's, the other set to -1.
pub(crate) fn replica_of(version: i16, req: &FetchRequest) -> i32 {
    if version < 15 {
        req.replica_id.0
    } else {
        req.replica_state.replica_id.0
    }
}

/// A partition a Fetch reads, or why it cannot.
pub(super) enum Place<F: Fs> {
    Error(ResponseError),
    Open(Arc<Partition<F>>),
}

/// One topic of the request, with its partitions resolved.
pub(super) struct Wanted<'a, F: Fs> {
    pub(super) request: &'a FetchTopic,
    pub(super) places: Vec<Place<F>>,
}

impl<F: Fs> Broker<F> {
    /// Reads each requested partition from its fetch offset. If the request's `min_bytes` is not met it waits, up
    /// to `max_wait_ms`, for a produce to any of the partitions, and answers with what has accumulated.
    ///
    /// Who may read what is decided when the partitions are resolved, before any is looked for (see
    /// [`Broker::resolve_all`]), so a refusal is an answer at once like any other error.
    pub(crate) async fn fetch(&self, version: i16, caller: &Caller, req: FetchRequest) -> Outcome {
        // Fetch sessions (KIP-227) are not kept: every response says session 0, so a conforming client keeps
        // sending full fetches. One that continues a session anyway is told it does not exist.
        if req.session_id != 0 && req.session_epoch > 0 {
            let mut resp = FetchResponse::default();
            resp.error_code = ResponseError::FetchSessionIdNotFound.code();
            return Outcome::Respond(Response::Fetch(resp));
        }
        let (mut wanted, mut watching) = self.resolve_and_watch(version, caller, &req).await;
        let min_bytes = usize::try_from(req.min_bytes.max(1)).unwrap_or(1);
        let waits =
            Duration::from_millis(u64::try_from(req.max_wait_ms).unwrap_or(0)).min(MAX_WAIT);
        let deadline = Instant::now() + waits;
        loop {
            let budget = Budget::new(req.max_bytes);
            let how = round::How {
                version,
                kafka: self.config.kafka_major,
            };
            let round = blocking_round(&wanted, &req, how, budget);
            // An error is an answer: the client has to act on it, and making it wait would cost a consumer's offset
            // reset a whole wait.
            if round.failed
                || round.bytes >= min_bytes
                || waits.is_zero()
                || Instant::now() >= deadline
            {
                return Outcome::Respond(Response::Fetch(round.into_response()));
            }
            // A change, or the deadline, then read again. A topic that changed means what was resolved is out of
            // date (a deleted one now answers as unknown).
            if let Ok(wait::Woke::Topics) = timeout_at(deadline, watching.changed()).await {
                (wanted, watching) = self.resolve_and_watch(version, caller, &req).await;
            }
        }
    }

    /// Finds the partitions the request names and starts watching them and the topics. The watch is made before the
    /// topics are looked at, and anew each time: a produce between the read and the wait would otherwise be missed
    /// (this fetch would sleep its whole wait with data already there), and a receiver kept from before a change
    /// would find that change again at once, every time, and spin.
    async fn resolve_and_watch<'a>(
        &self,
        version: i16,
        caller: &Caller,
        req: &'a FetchRequest,
    ) -> (Vec<Wanted<'a, F>>, wait::Watching) {
        let topics = self.topics_changed.subscribe();
        let wanted = self.resolve_all(version, caller, req).await;
        let watching = wait::Watching::new(&wanted, topics);
        (wanted, watching)
    }

    /// Finds every partition the request names, opening those that exist. A topic the caller may not read is
    /// answered 29 on each of its partitions, before the topic is looked for (`KafkaApis.handleFetchRequest`).
    async fn resolve_all<'a>(
        &self,
        version: i16,
        caller: &Caller,
        req: &'a FetchRequest,
    ) -> Vec<Wanted<'a, F>> {
        // A request from a follower (a broker id, not -1) is the cluster's, checked once for the whole request.
        let follower = (replica_of(version, req) >= 0)
            .then(|| self.cluster_allows(caller, Operation::ClusterAction));
        let mut out = Vec::new();
        for t in &req.topics {
            let topic = self.fetch_topic(version, t);
            let allowed =
                follower.unwrap_or_else(|| self.may_read(caller, version, t, topic.as_deref()));
            let mut places = Vec::new();
            for p in &t.partitions {
                places.push(if allowed {
                    self.place(version, topic.as_deref(), t, p.partition).await
                } else {
                    Place::Error(ResponseError::TopicAuthorizationFailed)
                });
            }
            out.push(Wanted { request: t, places });
        }
        out
    }

    /// Whether a consumer may READ the topic an entry names. From v13 the entry names it by id, and the check is on the
    /// name the id stands for: an id that stands for nothing is unknown to everyone, and not checked.
    fn may_read(
        &self,
        caller: &Caller,
        version: i16,
        t: &FetchTopic,
        found: Option<&Topic>,
    ) -> bool {
        let name = match found {
            Some(topic) => topic.name.as_ref(),
            None if version >= 13 => return true,
            None => t.topic.0.as_str(),
        };
        self.topic_allows(caller, name, Operation::Read)
    }

    fn fetch_topic(&self, version: i16, t: &FetchTopic) -> Option<Arc<Topic>> {
        if version >= 13 {
            return self.meta.topic_by_id(TopicId::from_uuid(t.topic_id)?);
        }
        self.meta.topic(&TopicName::parse(&t.topic.0).ok()?)
    }

    async fn place(
        &self,
        version: i16,
        topic: Option<&Topic>,
        t: &FetchTopic,
        index: i32,
    ) -> Place<F> {
        // A topic asked for by an id (the name is empty) that stands for none is UNKNOWN_TOPIC_ID; a partition the topic
        // has not is UNKNOWN_TOPIC_OR_PARTITION however the topic was named (Kafka 4.4.0, `tools/wire-tail-oracle`, `fetch`).
        let Some(topic) = topic else {
            return Place::Error(if t.topic.0.is_empty() {
                ResponseError::UnknownTopicId
            } else {
                ResponseError::UnknownTopicOrPartition
            });
        };
        let Some(id) = PartitionId::new(index).filter(|id| id.get() < topic.partitions.get())
        else {
            return Place::Error(ResponseError::UnknownTopicOrPartition);
        };
        if let Some(code) = self.refused_to_old_consumer(version, topic) {
            return Place::Error(code);
        }
        match self.partitions.get(topic, id).await {
            Ok(partition) => Place::Open(partition),
            Err(e) => {
                tracing::error!(error = %e, topic = %topic.name, partition = index, "a fetch could not open a partition");
                Place::Error(ResponseError::KafkaStorageError)
            }
        }
    }
}

impl<F: Fs> Broker<F> {
    /// Kafka's look at a topic an older consumer reads (`KafkaApis.maybeConvertFetchedData`, from Kafka 2.0): a topic
    /// that stores zstd is not read below Fetch v10, the first a consumer that knows zstd sends (KIP-110); and one whose
    /// `message.downconversion.enable` is false is not read below v4, which would need its records converted (KIP-283).
    fn refused_to_old_consumer(&self, version: i16, topic: &Topic) -> Option<ResponseError> {
        if self.config.kafka_major.get() < 2 {
            return None;
        }
        let settings = topic.settings();
        if version < ZSTD_READERS_FROM
            && settings.compression == Some(kafka_wire::batch::Compression::Zstd)
        {
            return Some(ResponseError::UnsupportedCompressionType);
        }
        (version < round::BATCHES_FROM && !settings.down_conversion)
            .then_some(ResponseError::UnsupportedVersion)
    }
}

/// The first Fetch version a consumer that reads zstd sends (KIP-110).
const ZSTD_READERS_FROM: i16 = 10;

fn blocking_round<F: Fs>(
    wanted: &[Wanted<'_, F>],
    req: &FetchRequest,
    how: round::How,
    budget: Budget,
) -> round::Round {
    crate::blocking::run(|| round::read(wanted, req, how, budget))
}

#[cfg(test)]
mod tests;
