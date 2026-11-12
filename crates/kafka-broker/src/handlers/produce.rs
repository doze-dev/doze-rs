//! Produce.

mod answer;
mod check;
mod request;

use std::sync::Arc;

use kafka_auth::authz::{operation, resource};
use kafka_meta::Topic;
use kafka_storage::fs::Fs;
use kafka_wire::api::Response;
use kafka_wire::batch::TimestampType;
use kafka_wire::ids::{PartitionId, TimestampMs, TopicId, TopicName};
use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_wire::messages::produce_response::{
    BatchIndexAndErrorMessage, PartitionProduceResponse, TopicProduceResponse,
};
use kafka_wire::messages::{ProduceRequest, ProduceResponse};
use kafka_wire::{ResponseError, StrBytes};

use crate::partitions::{Partition, Pending};
use crate::{Broker, Caller, Outcome, blocking, clock};
use answer::{blank, failed, finish, storage_fault};
use check::Accepted;
use request::{refused_everywhere, request_refusal};

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

/// What a Produce request carries of a transaction: the id it names, and the transaction, held for reading until the
/// request is answered (see [`Broker::hold_txn`]).
struct Txn<'a> {
    id: Option<&'a str>,
    held: Option<crate::txn::Held>,
}

impl<F: Fs> Broker<F> {
    /// Appends each partition's batch to its log. `acks=0` is not answered (and a connection that cannot be told
    /// it was refused is closed); `acks=1` is answered once the batch is written; `acks=-1` once it is flushed.
    ///
    /// Before any batch is looked at the caller must be allowed: to WRITE the transactional id if any batch is
    /// transactional (53 on every partition if not), and to WRITE each topic (29 on that topic's partitions if not,
    /// before the topic is looked for). Produce asks for nothing else: an idempotent producer needs no
    /// IDEMPOTENT_WRITE here (`KafkaApis.handleProduceRequest`).
    pub(crate) async fn produce(
        &self,
        version: i16,
        caller: &Caller,
        req: ProduceRequest,
    ) -> Outcome {
        let acks = req.acks;
        let may_write_txn = |id: &str| {
            self.authz
                .allows(caller, resource::TRANSACTIONAL_ID, id, operation::WRITE)
        };
        let response = match request_refusal(&req, may_write_txn) {
            Some((code, message)) => refused_everywhere(&req, code, message.as_deref()),
            None => self.produce_partitions(version, caller, &req).await,
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

    async fn produce_partitions(
        &self,
        version: i16,
        caller: &Caller,
        req: &ProduceRequest,
    ) -> ProduceResponse {
        // Every partition's batch is queued with its writer before any answer is waited for, so the writers work
        // at once and each flushes a group.
        let durable = req.acks == -1;
        let id = req.transactional_id.as_ref().map(|t| t.0.as_str());
        let txn = Txn {
            id,
            held: self.hold_txn(id).await,
        };
        let mut staged = Vec::new();
        for t in &req.topic_data {
            let topic = self.topic_of(version, t.topic_id, &t.name);
            let allowed = self.may_write(caller, version, t, topic.as_deref());
            let mut stages = Vec::new();
            for p in &t.partition_data {
                stages.push(if allowed {
                    self.start(version, durable, &txn, topic.as_deref(), p)
                        .await
                } else {
                    let code = ResponseError::TopicAuthorizationFailed;
                    Stage::Done(failed(p.index, code, None))
                });
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

    /// Whether the caller may WRITE the topic a request entry names, asked once for the entry and before the topic
    /// is looked for. From v13 a topic is named by id and the check is on the name the id stands for, so an id that
    /// stands for nothing is not checked: it is unknown to everyone.
    fn may_write(
        &self,
        caller: &Caller,
        version: i16,
        t: &TopicProduceData,
        found: Option<&Topic>,
    ) -> bool {
        let name = match found {
            Some(topic) => topic.name.as_ref(),
            None if version >= 13 => return true,
            None => t.name.0.as_str(),
        };
        self.topic_allows(caller, name, operation::WRITE)
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
        txn: &Txn<'_>,
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
        let now = clock::now();
        let records = p.records.clone().unwrap_or_default();
        let Accepted {
            batch,
            log_append_time,
        } = match blocking::run(|| {
            check::check(&self.defaults.get(), version, topic, id, &records, now)
        }) {
            Ok(accepted) => accepted,
            Err(refused) => return Stage::Done(self.refused(topic, id, refused).await),
        };
        // A transactional batch is part of a transaction that must be open for it. (A transactional bit on a batch with no
        // producer is stored as any other: Kafka does.)
        if batch.attributes().is_transactional()
            && let Some(producer) = batch.producer()
        {
            match Self::check_txn_batch(
                txn.id,
                txn.held.as_ref(),
                (producer.id.get(), producer.epoch.get()),
                (topic.name.as_ref(), p.index),
            ) {
                Ok(()) => {}
                Err((code, why)) => {
                    return Stage::Done(failed(p.index, code, Some(why.to_owned())));
                }
            }
        }
        let partition = match self.partitions.get(topic, id).await {
            Ok(partition) => partition,
            Err(e) => return Stage::Done(storage_fault(p.index, &e)),
        };
        let clock_kind = if log_append_time {
            TimestampType::LogAppend
        } else {
            TimestampType::Create
        };
        match partition.submit(batch, now, clock_kind, durable).await {
            Ok(pending) => Stage::Waiting {
                answer: blank(p.index),
                partition,
                pending,
                appended_at: log_append_time.then_some(now),
            },
            Err(e) => Stage::Done(storage_fault(p.index, &e)),
        }
    }

    /// The answer for a batch that was refused. Kafka's carries the partition's log start offset once the log has
    /// been asked, and -1 for a refusal made before that.
    async fn refused(
        &self,
        topic: &Topic,
        id: PartitionId,
        why: check::Refused,
    ) -> PartitionProduceResponse {
        let mut r = failed(id.get(), why.code, why.message);
        r.record_errors = why
            .records
            .into_iter()
            .map(|e| {
                let mut one = BatchIndexAndErrorMessage::default();
                one.batch_index = e.index;
                one.batch_index_error_message = Some(StrBytes::from_string(e.message));
                one
            })
            .collect();
        if !why.before_log
            && let Ok(partition) = self.partitions.get(topic, id).await
        {
            r.log_start_offset = partition.reader().start_offset().get();
        }
        r
    }
}
