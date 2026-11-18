//! AddPartitionsToTxn: what a transaction will write to. Versions 0 to 3 are a client's, for one transaction; from
//! version 4 (KIP-890) the request is a broker's, batching several transactions, each either verified (is the partition
//! in the transaction?) or added to.

use kafka_auth::authz::Operation;
use kafka_meta::TxnVersion;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::api::Response;
use kafka_wire::ids::TopicName;
use kafka_wire::messages::add_partitions_to_txn_request::{
    AddPartitionsToTxnTopic, AddPartitionsToTxnTransaction,
};
use kafka_wire::messages::add_partitions_to_txn_response::{
    AddPartitionsToTxnPartitionResult, AddPartitionsToTxnResult, AddPartitionsToTxnTopicResult,
};
use kafka_wire::messages::{AddPartitionsToTxnRequest, AddPartitionsToTxnResponse};

use crate::handlers::group_guard::Guard;
use crate::txn::rules::{self, Adding};
use crate::{Broker, Caller, Outcome};

/// A topic's partitions with the code each was answered (`None`: added, or verified).
type Judged = Vec<(kafka_wire::messages::TopicName, Vec<(i32, Option<E>)>)>;

/// Every partition of `topics` answered `code`.
fn everywhere(topics: &[AddPartitionsToTxnTopic], code: E) -> Judged {
    topics
        .iter()
        .map(|t| {
            (
                t.name.clone(),
                t.partitions.iter().map(|p| (*p, Some(code))).collect(),
            )
        })
        .collect()
}

impl<F: Fs> Broker<F> {
    /// Versions 0 to 3: the caller must be allowed to WRITE the transactional id (53 on every partition if not), and each
    /// topic (29 on its partitions, before it is looked for, and then the others are not attempted). From version 4 the
    /// request is a broker's: the caller must be allowed CLUSTER_ACTION on the cluster, or the whole request is answered
    /// CLUSTER_AUTHORIZATION_FAILED with no transaction in it, and nothing else is asked of the authorizer
    /// (`KafkaApis.handleAddPartitionsToTxnRequest`).
    pub(crate) async fn add_partitions_to_txn(
        &self,
        version: i16,
        caller: &Caller,
        req: &AddPartitionsToTxnRequest,
    ) -> Outcome {
        let mut resp = AddPartitionsToTxnResponse::default();
        if version >= 4 {
            if self.guard(caller).cluster(Operation::ClusterAction) {
                for t in &req.transactions {
                    let judged = self.judge_transaction(t).await;
                    let mut r = AddPartitionsToTxnResult::default();
                    r.transactional_id = t.transactional_id.clone();
                    r.topic_results = judged.into_iter().map(result).collect();
                    resp.results_by_transaction.push(r);
                }
            } else {
                resp.error_code = E::ClusterAuthorizationFailed.code();
            }
        } else {
            let judged = self.judge_additions(version, caller, req).await;
            resp.results_by_topic_v3_and_below = judged.into_iter().map(result).collect();
        }
        Outcome::Respond(Response::AddPartitionsToTxn(resp))
    }

    /// Each partition of `topics` judged by whether it exists (and, for a client, may be written), and those to add:
    /// `None` if any was refused.
    fn judge_partitions(
        &self,
        guard: Option<&Guard<'_>>,
        topics: &[AddPartitionsToTxnTopic],
    ) -> (Judged, Option<Vec<(String, i32)>>) {
        let mut add: Vec<(String, i32)> = Vec::new();
        let mut judged: Judged = Vec::new();
        for t in topics {
            let topic = TopicName::parse(&t.name.0)
                .ok()
                .and_then(|n| self.meta.topic(&n));
            let barred = guard.is_some_and(|g| !g.topic(Operation::Write, t.name.0.as_str()));
            let mut codes = Vec::new();
            for &p in &t.partitions {
                let known = !barred
                    && topic
                        .as_ref()
                        .is_some_and(|t| p >= 0 && p < t.partitions.get());
                if known {
                    add.push((t.name.0.to_string(), p));
                }
                let refusal = if barred {
                    Some(E::TopicAuthorizationFailed)
                } else {
                    (!known).then_some(E::UnknownTopicOrPartition)
                };
                codes.push((p, refusal));
            }
            judged.push((t.name.clone(), codes));
        }
        // One partition that cannot be added leaves the rest not attempted, and the transaction is not looked at.
        if judged
            .iter()
            .any(|(_, codes)| codes.iter().any(|(_, c)| c.is_some()))
        {
            for (_, codes) in &mut judged {
                for (_, c) in codes {
                    c.get_or_insert(E::OperationNotAttempted);
                }
            }
            return (judged, None);
        }
        (judged, Some(add))
    }

    /// Every partition of a client's request (v0 to v3), with its answer.
    async fn judge_additions(
        &self,
        version: i16,
        caller: &Caller,
        req: &AddPartitionsToTxnRequest,
    ) -> Judged {
        let guard = self.guard(caller);
        let id = req.v3_and_below_transactional_id.0.as_str();
        let topics = &req.v3_and_below_topics;
        if !guard.txn(Operation::Write, id) {
            return everywhere(topics, E::TransactionalIdAuthorizationFailed);
        }
        // Every partition is judged before the coordinator is asked anything, as in Kafka.
        let (judged, add) = self.judge_partitions(Some(&guard), topics);
        let Some(add) = add else { return judged };
        let who = (
            req.v3_and_below_producer_id.0,
            req.v3_and_below_producer_epoch,
        );
        match self.add_to_txn(id, who, add, TxnVersion::Classic).await {
            Ok(()) => judged,
            Err(code) => everywhere(topics, rules::fenced_as(code, version, 2)),
        }
    }

    /// One transaction of a broker's request (v4 on): verified, or added to at the second transaction version
    /// (`TransactionVersion.transactionVersionForAddPartitionsToTxn`: above v3, `TV_2`).
    async fn judge_transaction(&self, t: &AddPartitionsToTxnTransaction) -> Judged {
        let (judged, add) = self.judge_partitions(None, &t.topics);
        let Some(add) = add else { return judged };
        let id = t.transactional_id.0.as_str();
        let who = (t.producer_id.0, t.producer_epoch);
        if t.verify_only {
            return self.verify(id, who, &t.topics).await;
        }
        match self.add_to_txn(id, who, add, TxnVersion::EpochBump).await {
            Ok(()) => judged,
            Err(code) => everywhere(&t.topics, code),
        }
    }

    /// The coordinator's add: the producer must own the id and the transaction must not be being ended; what is not in the
    /// open transaction is added (one is begun if none is open) and stored before the answer.
    pub(crate) async fn add_to_txn(
        &self,
        id: &str,
        (producer, epoch): (i64, i16),
        add: Vec<(String, i32)>,
        version: TxnVersion,
    ) -> Result<(), E> {
        if id.is_empty() {
            return Err(E::InvalidRequest);
        }
        let Some(mut e) = self.txns.lock(id).await else {
            return Err(E::InvalidProducerIdMapping);
        };
        let there = add.iter().all(|(t, p)| rules::holds(&e.rec, (t, *p)));
        match rules::may_add(&e.rec, producer, epoch, there) {
            Adding::Refused(code) => Err(code),
            Adding::There => Ok(()),
            Adding::Store => {
                let now = crate::clock::now().get();
                self.update_txn(&mut e, |rec| rules::add(rec, add, version, now))
                    .await
            }
        }
    }

    /// The coordinator's verification (`handleVerifyPartitionsInTransaction`, the `verify_only` of v4 on): each partition
    /// is NONE if the transaction has it and TRANSACTION_ABORTABLE if not; nothing is changed. An empty id is
    /// INVALID_REQUEST, an id nobody owns or another producer id INVALID_PRODUCER_ID_MAPPING, another epoch
    /// PRODUCER_FENCED, and a transaction being ended CONCURRENT_TRANSACTIONS, on every partition.
    async fn verify(
        &self,
        id: &str,
        (producer, epoch): (i64, i16),
        topics: &[AddPartitionsToTxnTopic],
    ) -> Judged {
        if id.is_empty() {
            return everywhere(topics, E::InvalidRequest);
        }
        let Some(entry) = self.txns.get(id) else {
            return everywhere(topics, E::InvalidProducerIdMapping);
        };
        let e = entry.read().await;
        if let Some(code) = rules::owns(&e.rec, producer, epoch).or_else(|| rules::can_add(&e.rec))
        {
            return everywhere(topics, code);
        }
        topics
            .iter()
            .map(|t| {
                let codes = t
                    .partitions
                    .iter()
                    .map(|&p| {
                        let held = rules::holds(&e.rec, (t.name.0.as_str(), p));
                        (p, (!held).then_some(E::TransactionAbortable))
                    })
                    .collect();
                (t.name.clone(), codes)
            })
            .collect()
    }
}

fn result(
    (name, codes): (kafka_wire::messages::TopicName, Vec<(i32, Option<E>)>),
) -> AddPartitionsToTxnTopicResult {
    let mut t = AddPartitionsToTxnTopicResult::default();
    t.name = name;
    t.results_by_partition = codes
        .into_iter()
        .map(|(p, c)| {
            let mut r = AddPartitionsToTxnPartitionResult::default();
            r.partition_index = p;
            r.partition_error_code = c.map_or(0, |c| c.code());
            r
        })
        .collect();
    t
}
