//! AddPartitionsToTxn (the shape of versions 0 to 3: one transaction): what the transaction will write to.

use kafka_auth::authz::operation;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::api::Response;
use kafka_wire::ids::TopicName;
use kafka_wire::messages::add_partitions_to_txn_response::{
    AddPartitionsToTxnPartitionResult, AddPartitionsToTxnTopicResult,
};
use kafka_wire::messages::{AddPartitionsToTxnRequest, AddPartitionsToTxnResponse};

use crate::handlers::group_guard::Guard;
use crate::txn::rules;
use crate::{Broker, Caller, Outcome};

/// A topic's partitions with the code each was answered (`None`: added).
type Judged = Vec<(kafka_wire::messages::TopicName, Vec<(i32, Option<E>)>)>;

impl<F: Fs> Broker<F> {
    /// The caller must be allowed to WRITE the transactional id (53 on every partition if not), and each topic (29 on
    /// its partitions, before it is looked for, and then the others are not attempted).
    pub(crate) async fn add_partitions_to_txn(
        &self,
        version: i16,
        caller: &Caller,
        req: &AddPartitionsToTxnRequest,
    ) -> Outcome {
        let mut resp = AddPartitionsToTxnResponse::default();
        let judged = self.judge_additions(version, caller, req).await;
        resp.results_by_topic_v3_and_below = judged
            .into_iter()
            .map(|(name, codes)| result(name, codes))
            .collect();
        Outcome::Respond(Response::AddPartitionsToTxn(resp))
    }

    /// Each partition of the request judged by whether it exists, and those that do.
    fn judge_partitions(
        &self,
        guard: &Guard<'_>,
        req: &AddPartitionsToTxnRequest,
    ) -> (Judged, Vec<(String, i32)>) {
        let mut add: Vec<(String, i32)> = Vec::new();
        let mut judged: Judged = Vec::new();
        for t in &req.v3_and_below_topics {
            let topic = TopicName::parse(&t.name.0)
                .ok()
                .and_then(|n| self.meta.topic(&n));
            let barred = !guard.topic(operation::WRITE, t.name.0.as_str());
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
        (judged, add)
    }

    /// Every partition of the request, with its answer.
    async fn judge_additions(
        &self,
        version: i16,
        caller: &Caller,
        req: &AddPartitionsToTxnRequest,
    ) -> Judged {
        let guard = self.guard(caller);
        let id = req.v3_and_below_transactional_id.0.as_str();
        let everywhere = |code: E| -> Judged {
            req.v3_and_below_topics
                .iter()
                .map(|t| {
                    (
                        t.name.clone(),
                        t.partitions.iter().map(|p| (*p, Some(code))).collect(),
                    )
                })
                .collect()
        };
        if !guard.txn(operation::WRITE, id) {
            return everywhere(E::TransactionalIdAuthorizationFailed);
        }
        // Every partition is judged before the coordinator is asked anything: one that cannot be added leaves the rest
        // not attempted, and the transaction is not looked at, as in Kafka.
        let (mut judged, add) = self.judge_partitions(&guard, req);
        if judged
            .iter()
            .any(|(_, codes)| codes.iter().any(|(_, c)| c.is_some()))
        {
            for (_, codes) in &mut judged {
                for (_, c) in codes {
                    c.get_or_insert(E::OperationNotAttempted);
                }
            }
            return judged;
        }
        let everywhere = |code: E| everywhere(rules::fenced_as(code, version, 2));
        if id.is_empty() {
            return everywhere(E::InvalidRequest);
        }
        let Some(mut e) = self.txns.lock(id).await else {
            return everywhere(E::InvalidProducerIdMapping);
        };
        if let Some(code) = rules::owns(
            &e.rec,
            req.v3_and_below_producer_id.0,
            req.v3_and_below_producer_epoch,
        )
        .or_else(|| rules::can_add(&e.rec))
        {
            return everywhere(code);
        }
        let now = crate::clock::now().get();
        let stored = self
            .update_txn(&mut e, |rec| {
                rules::begin(rec, now);
                for a in add {
                    if !rec.partitions.contains(&a) {
                        rec.partitions.push(a);
                    }
                }
            })
            .await;
        match stored {
            Ok(()) => judged,
            Err(code) => everywhere(code),
        }
    }
}

fn result(
    name: kafka_wire::messages::TopicName,
    codes: Vec<(i32, Option<E>)>,
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
