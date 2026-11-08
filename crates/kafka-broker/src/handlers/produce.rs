//! Produce.

use std::sync::Arc;

use kafka_meta::Topic;
use kafka_storage::fs::Fs;
use kafka_wire::api::Response;
use kafka_wire::batch::{Batch, ProduceError, ProduceRules, Refusal, TimestampType};
use kafka_wire::ids::{PartitionId, TimestampMs, TopicId, TopicName};
use kafka_wire::messages::produce_request::PartitionProduceData;
use kafka_wire::messages::produce_response::{PartitionProduceResponse, TopicProduceResponse};
use kafka_wire::messages::{ProduceRequest, ProduceResponse};
use kafka_wire::{ResponseError, StrBytes};

use crate::partitions::{Partition, Pending, gone};
use crate::{Broker, Outcome, clock};

/// A partition's answer, or what it is waiting for.
enum Stage<F: Fs> {
    Done(PartitionProduceResponse),
    Waiting {
        answer: PartitionProduceResponse,
        partition: Arc<Partition<F>>,
        pending: Pending,
        appended_at: Option<TimestampMs>,
    },
}

type Refused = (ResponseError, Option<String>);

impl<F: Fs> Broker<F> {
    /// Appends each partition's batch to its log. `acks=0` is not answered (and a connection that cannot be told
    /// it was refused is closed); `acks=1` is answered once the batch is written; `acks=-1` once it is flushed.
    pub(crate) async fn produce(&self, version: i16, req: ProduceRequest) -> Outcome {
        let acks = req.acks;
        let response = match request_refusal(&req) {
            Some((code, message)) => refused_everywhere(&req, code, message.as_deref()),
            None => self.produce_partitions(version, &req).await,
        };
        if acks == 0 {
            // No answer can carry an error, so a refusal is told by dropping the connection, as Kafka does.
            let failed = response
                .responses
                .iter()
                .flat_map(|t| &t.partition_responses)
                .any(|p| p.error_code != 0);
            return if failed {
                Outcome::Close
            } else {
                Outcome::NoResponse
            };
        }
        Outcome::Respond(Response::Produce(response))
    }

    async fn produce_partitions(&self, version: i16, req: &ProduceRequest) -> ProduceResponse {
        // Every partition's batch is queued with its writer before any answer is waited for, so the writers work
        // at once and each flushes a group.
        let durable = req.acks == -1;
        let mut staged = Vec::new();
        for t in &req.topic_data {
            let topic = self.topic_of(version, t.topic_id, &t.name);
            let mut stages = Vec::new();
            for p in &t.partition_data {
                stages.push(self.start(version, durable, topic.as_deref(), p).await);
            }
            staged.push((t, stages));
        }
        let mut resp = ProduceResponse::default();
        for (t, stages) in staged {
            let mut r = TopicProduceResponse::default();
            r.name = t.name.clone();
            r.topic_id = t.topic_id;
            for stage in stages {
                r.partition_responses.push(finish(stage, durable).await);
            }
            resp.responses.push(r);
        }
        resp
    }

    /// The topic a request names: by id from v13, by name before.
    fn topic_of(
        &self,
        version: i16,
        id: kafka_wire::Uuid,
        name: &kafka_wire::messages::TopicName,
    ) -> Option<Arc<Topic>> {
        if version >= 13 {
            return self.meta.topic_by_id(TopicId::from_uuid(id)?);
        }
        self.meta.topic(&TopicName::parse(&name.0).ok()?)
    }

    /// Checks one partition's batch and queues it.
    async fn start(
        &self,
        version: i16,
        durable: bool,
        topic: Option<&Topic>,
        p: &PartitionProduceData,
    ) -> Stage<F> {
        let unknown = || {
            Stage::Done(failed(
                p.index,
                ResponseError::UnknownTopicOrPartition,
                None,
            ))
        };
        let (Some(topic), Some(id)) = (topic, PartitionId::new(p.index)) else {
            return unknown();
        };
        if id.get() >= topic.partitions.get() {
            return unknown();
        }
        let rules = ProduceRules {
            version,
            max_batch_bytes: self.max_batch_bytes(topic),
        };
        let records = p.records.clone().unwrap_or_default();
        let batch = match Batch::from_produce(&records, rules) {
            Ok(batch) => batch,
            Err(e) => return Stage::Done(self.refused_batch(topic, id, &e).await),
        };
        if batch.attributes().is_transactional() {
            // Transactions are Phase 6; a producer that is not in one has no business sending these.
            return Stage::Done(failed(p.index, ResponseError::InvalidTxnState, None));
        }
        let partition = match self.partitions.get(topic, id).await {
            Ok(partition) => partition,
            Err(e) => return Stage::Done(storage_fault(p.index, &e)),
        };
        let log_append = topic
            .config
            .get("message.timestamp.type")
            .is_some_and(|v| v == "LogAppendTime");
        let (clock_kind, now) = (
            if log_append {
                TimestampType::LogAppend
            } else {
                TimestampType::Create
            },
            clock::now(),
        );
        match partition.submit(batch, now, clock_kind, durable).await {
            Ok(pending) => Stage::Waiting {
                answer: blank(p.index),
                partition,
                pending,
                appended_at: log_append.then_some(now),
            },
            Err(e) => Stage::Done(storage_fault(p.index, &e)),
        }
    }

    fn max_batch_bytes(&self, topic: &Topic) -> u64 {
        let own = topic
            .config
            .get("max.message.bytes")
            .and_then(|v| v.parse::<i64>().ok());
        u64::try_from(own.unwrap_or(self.config.defaults.message_max_bytes)).unwrap_or(0)
    }

    /// The answer for a batch that was refused. Kafka's carries the partition's log start offset once the log has
    /// been asked, and -1 for a refusal made before that.
    async fn refused_batch(
        &self,
        topic: &Topic,
        id: PartitionId,
        e: &ProduceError,
    ) -> PartitionProduceResponse {
        let code = match e.refusal() {
            Refusal::CorruptMessage => ResponseError::CorruptMessage,
            Refusal::InvalidRecord => ResponseError::InvalidRecord,
            Refusal::MessageTooLarge => ResponseError::MessageTooLarge,
            Refusal::UnsupportedCompression => ResponseError::UnsupportedCompressionType,
            _ => ResponseError::UnknownServerError,
        };
        let mut r = failed(id.get(), code, e.kafka_message());
        if !e.before_log()
            && let Ok(partition) = self.partitions.get(topic, id).await
        {
            r.log_start_offset = partition.reader().start_offset().get();
        }
        r
    }
}

/// Waits for a queued append to be written (and, for `acks=-1`, flushed) and builds its answer.
async fn finish<F: Fs>(stage: Stage<F>, durable: bool) -> PartitionProduceResponse {
    match stage {
        Stage::Done(done) => done,
        Stage::Waiting {
            answer,
            partition,
            pending,
            appended_at,
        } => wait_for(answer, &partition, pending, appended_at, durable).await,
    }
}

async fn wait_for<F: Fs>(
    mut answer: PartitionProduceResponse,
    partition: &Partition<F>,
    pending: Pending,
    appended_at: Option<TimestampMs>,
    durable: bool,
) -> PartitionProduceResponse {
    let appended = match pending.await {
        Ok(Ok(appended)) => appended,
        Ok(Err(e)) => return storage_fault(answer.index, &e),
        Err(_) => return storage_fault(answer.index, &gone()),
    };
    if durable {
        // acks=all is a promise that the record is on disk: if the disk says otherwise the producer hears it.
        if let Err(e) = appended.ticket.durable().await {
            return storage_fault(answer.index, &e);
        }
    }
    answer.base_offset = appended.first_offset.get();
    answer.log_append_time_ms = appended_at.map_or(-1, TimestampMs::get);
    answer.log_start_offset = partition.reader().start_offset().get();
    answer
}

/// An answer with no error and no offsets yet.
fn blank(index: i32) -> PartitionProduceResponse {
    let mut r = PartitionProduceResponse::default();
    r.index = index;
    r.base_offset = -1;
    r.log_append_time_ms = -1;
    r.log_start_offset = -1;
    r.error_message = None;
    r
}

/// An answer that carries an error and no offsets.
fn failed(index: i32, code: ResponseError, message: Option<String>) -> PartitionProduceResponse {
    let mut r = blank(index);
    r.error_code = code.code();
    r.error_message = message.map(StrBytes::from_string);
    r
}

/// A write or flush that failed, as Kafka answers a full disk: the code, no offsets and no message (the cause is
/// in the broker's log, not in a client's).
fn storage_fault(index: i32, e: &kafka_storage::StorageError) -> PartitionProduceResponse {
    tracing::error!(error = %e, partition = index, "a produce could not be stored");
    failed(index, ResponseError::KafkaStorageError, None)
}

/// What fails the whole request before any partition is looked at: a \`required acks\` that is not -1, 0 or 1; and
/// Kafka's look at every partition's first batch, which fails a request it cannot take apart there and one that has
/// a transactional batch and no transactional id.
fn request_refusal(req: &ProduceRequest) -> Option<Refused> {
    if !(-1..=1).contains(&req.acks) {
        return Some((ResponseError::InvalidRequiredAcks, None));
    }
    let mut transactional = false;
    for p in req.topic_data.iter().flat_map(|t| &t.partition_data) {
        let records = p.records.clone().unwrap_or_default();
        match Batch::first_is_transactional(&records) {
            Ok(t) => transactional |= t,
            Err(e) => return Some((code_of(&e), e.kafka_message())),
        }
    }
    (transactional && req.transactional_id.is_none())
        .then_some((ResponseError::TransactionalIdAuthorizationFailed, None))
}

fn code_of(e: &ProduceError) -> ResponseError {
    match e.refusal() {
        Refusal::CorruptMessage => ResponseError::CorruptMessage,
        _ => ResponseError::InvalidRecord,
    }
}

fn refused_everywhere(
    req: &ProduceRequest,
    code: ResponseError,
    message: Option<&str>,
) -> ProduceResponse {
    let mut resp = ProduceResponse::default();
    for t in &req.topic_data {
        let mut r = TopicProduceResponse::default();
        r.name = t.name.clone();
        r.topic_id = t.topic_id;
        for p in &t.partition_data {
            r.partition_responses
                .push(failed(p.index, code, message.map(str::to_owned)));
        }
        resp.responses.push(r);
    }
    resp
}
