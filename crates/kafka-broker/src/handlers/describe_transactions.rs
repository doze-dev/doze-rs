//! DescribeTransactions and ListTransactions: what the coordinator knows of its transactional ids, for tooling.

use std::collections::BTreeMap;

use kafka_meta::TxnState;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::StrBytes;
use kafka_wire::api::Response;
use kafka_wire::messages::describe_transactions_response::{TopicData, TransactionState};
use kafka_wire::messages::{
    DescribeTransactionsRequest, DescribeTransactionsResponse, ListTransactionsRequest,
    ListTransactionsResponse, ProducerId, TopicName, TransactionalId,
};

use crate::{Broker, Outcome};

impl<F: Fs> Broker<F> {
    pub(crate) async fn describe_transactions(&self, req: &DescribeTransactionsRequest) -> Outcome {
        let mut resp = DescribeTransactionsResponse::default();
        for id in &req.transactional_ids {
            let mut s = TransactionState::default();
            s.transactional_id = id.clone();
            s.producer_id = ProducerId(-1);
            s.transaction_start_time_ms = -1;
            match self.txns.get(id.0.as_str()) {
                Some(entry) if entry.read().await.rec.producer_id >= 0 => {
                    let e = entry.read().await;
                    let rec = &e.rec;
                    s.transaction_state = StrBytes::from_static_str(rec.state.name());
                    s.transaction_timeout_ms = rec.timeout_ms;
                    s.producer_id = ProducerId(rec.producer_id);
                    s.producer_epoch = rec.epoch;
                    let open = matches!(
                        rec.state,
                        TxnState::Ongoing | TxnState::PrepareCommit | TxnState::PrepareAbort
                    );
                    if open {
                        s.transaction_start_time_ms = rec.started_ms;
                    }
                    let mut by_topic: BTreeMap<&str, Vec<i32>> = BTreeMap::new();
                    for (topic, partition) in &rec.partitions {
                        by_topic.entry(topic.as_str()).or_default().push(*partition);
                    }
                    // A group whose offsets it commits is a partition of the offsets topic, which is written to.
                    for group in &rec.groups {
                        by_topic
                            .entry("__consumer_offsets")
                            .or_default()
                            .push(crate::groups::offsets_partition(group));
                    }
                    s.topics = by_topic
                        .into_iter()
                        .map(|(topic, partitions)| {
                            let mut t = TopicData::default();
                            t.topic = TopicName(StrBytes::from_string(topic.to_owned()));
                            t.partitions = partitions;
                            t
                        })
                        .collect();
                }
                _ => s.error_code = E::TransactionalIdNotFound.code(),
            }
            resp.transaction_states.push(s);
        }
        Outcome::Respond(Response::DescribeTransactions(resp))
    }

    pub(crate) async fn list_transactions(&self, req: &ListTransactionsRequest) -> Outcome {
        use kafka_wire::messages::list_transactions_response::TransactionState as Listed;
        let mut resp = ListTransactionsResponse::default();
        let mut rows: Vec<Listed> = Vec::new();
        for entry in self.txns.all() {
            let e = entry.read().await;
            let state = e.rec.state.name();
            let wanted_state = req.state_filters.is_empty()
                || req.state_filters.iter().any(|f| f.as_str() == state);
            let wanted_producer = req.producer_id_filters.is_empty()
                || req
                    .producer_id_filters
                    .iter()
                    .any(|p| p.0 == e.rec.producer_id);
            // (Kafka counts a transaction that is not open as begun at -1, so it has run for ever.)
            let began = if e.rec.state.is_open() {
                e.rec.started_ms
            } else {
                -1
            };
            let long_enough = req.duration_filter < 0
                || crate::clock::now().get().saturating_sub(began) > req.duration_filter;
            if e.rec.producer_id < 0 || !wanted_state || !wanted_producer || !long_enough {
                continue;
            }
            let mut row = Listed::default();
            row.transactional_id = TransactionalId(StrBytes::from_string(e.rec.id.clone()));
            row.producer_id = ProducerId(e.rec.producer_id);
            row.transaction_state = StrBytes::from_static_str(state);
            rows.push(row);
        }
        rows.sort_by(|a, b| {
            a.transactional_id
                .0
                .as_str()
                .cmp(b.transactional_id.0.as_str())
        });
        resp.transaction_states = rows;
        Outcome::Respond(Response::ListTransactions(resp))
    }
}
