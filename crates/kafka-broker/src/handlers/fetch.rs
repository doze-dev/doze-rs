//! Fetch.

mod round;
mod wait;

use std::sync::Arc;
use std::time::Duration;

use kafka_meta::Topic;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::api::Response;
use kafka_wire::ids::{PartitionId, TopicId, TopicName};
use kafka_wire::messages::fetch_request::FetchTopic;
use kafka_wire::messages::{FetchRequest, FetchResponse};
use tokio::time::{Instant, timeout_at};

use crate::partitions::Partition;
use crate::{Broker, Outcome};
use round::Budget;

/// The longest a Fetch waits for data, whatever the client asked for. A wait is inside a connection that cannot
/// notice its client going away until it writes the answer, so an unbounded one would hold a socket for as long as
/// the client's timeout, and answering early with nothing is always correct: the client fetches again.
const MAX_WAIT: Duration = Duration::from_secs(30);

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
    pub(crate) async fn fetch(&self, version: i16, req: FetchRequest) -> Outcome {
        // Fetch sessions (KIP-227) are not kept: every response says session 0, so a conforming client keeps
        // sending full fetches. One that continues a session anyway is told it does not exist.
        if req.session_id != 0 && req.session_epoch > 0 {
            let mut resp = FetchResponse::default();
            resp.error_code = ResponseError::FetchSessionIdNotFound.code();
            return Outcome::Respond(Response::Fetch(resp));
        }
        let (mut wanted, mut watching) = self.resolve_and_watch(version, &req).await;
        let min_bytes = usize::try_from(req.min_bytes.max(1)).unwrap_or(1);
        let waits =
            Duration::from_millis(u64::try_from(req.max_wait_ms).unwrap_or(0)).min(MAX_WAIT);
        let deadline = Instant::now() + waits;
        loop {
            let budget = Budget::new(req.max_bytes);
            let round = blocking_round(&wanted, &req, budget);
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
                (wanted, watching) = self.resolve_and_watch(version, &req).await;
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
        req: &'a FetchRequest,
    ) -> (Vec<Wanted<'a, F>>, wait::Watching) {
        let topics = self.topics_changed.subscribe();
        let wanted = self.resolve_all(version, req).await;
        let watching = wait::Watching::new(&wanted, topics);
        (wanted, watching)
    }

    /// Finds every partition the request names, opening those that exist.
    async fn resolve_all<'a>(&self, version: i16, req: &'a FetchRequest) -> Vec<Wanted<'a, F>> {
        let mut out = Vec::new();
        for t in &req.topics {
            let topic = self.fetch_topic(version, t);
            let mut places = Vec::new();
            for p in &t.partitions {
                places.push(self.place(topic.as_deref(), t, p.partition).await);
            }
            out.push(Wanted { request: t, places });
        }
        out
    }

    fn fetch_topic(&self, version: i16, t: &FetchTopic) -> Option<Arc<Topic>> {
        if version >= 13 {
            return self.meta.topic_by_id(TopicId::from_uuid(t.topic_id)?);
        }
        self.meta.topic(&TopicName::parse(&t.topic.0).ok()?)
    }

    async fn place(&self, topic: Option<&Topic>, t: &FetchTopic, index: i32) -> Place<F> {
        // Asked for by id (the name is empty) or by name decides which "unknown" it is.
        let unknown = if t.topic.0.is_empty() {
            ResponseError::UnknownTopicId
        } else {
            ResponseError::UnknownTopicOrPartition
        };
        let (Some(topic), Some(id)) = (topic, PartitionId::new(index)) else {
            return Place::Error(unknown);
        };
        if id.get() >= topic.partitions.get() {
            return Place::Error(unknown);
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

fn blocking_round<F: Fs>(
    wanted: &[Wanted<'_, F>],
    req: &FetchRequest,
    budget: Budget,
) -> round::Round {
    crate::blocking::run(|| round::read(wanted, req, budget))
}

#[cfg(test)]
mod tests;
