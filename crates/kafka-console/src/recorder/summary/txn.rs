//! Transactions and the producer and transaction readers.

use kafka_wire::messages::{
    AddOffsetsToTxnRequest, AddOffsetsToTxnResponse, AddPartitionsToTxnRequest,
    AddPartitionsToTxnResponse, DescribeProducersRequest, DescribeProducersResponse,
    DescribeTransactionsRequest, DescribeTransactionsResponse, EndTxnRequest, EndTxnResponse,
    InitProducerIdRequest, InitProducerIdResponse, ListTransactionsRequest,
    ListTransactionsResponse, TxnOffsetCommitRequest, TxnOffsetCommitResponse,
};

use super::n;
use crate::recorder::facts::Facts;

pub(super) fn init_request(r: &InitProducerIdRequest, f: &mut Facts) {
    if let Some(id) = &r.transactional_id {
        f.txn(&id.0);
    }
    if r.producer_id.0 >= 0 {
        f.producer = Some((r.producer_id.0, r.producer_epoch));
    }
}

pub(super) fn init_response(r: &InitProducerIdResponse, f: &mut Facts) {
    f.error(r.error_code);
    if r.error_code == 0 {
        // The identity the broker gave: the epoch a fenced producer is later told it is behind.
        f.producer = Some((r.producer_id.0, r.producer_epoch));
    }
}

pub(super) fn add_partitions_request(r: &AddPartitionsToTxnRequest, f: &mut Facts) {
    f.txn(&r.v3_and_below_transactional_id.0);
    if !r.v3_and_below_transactional_id.0.is_empty() {
        f.producer = Some((r.v3_and_below_producer_id.0, r.v3_and_below_producer_epoch));
    }
    for t in &r.v3_and_below_topics {
        f.topic(&t.name.0, t.partitions.iter().copied());
    }
    for tx in &r.transactions {
        f.txn(&tx.transactional_id.0);
        f.producer = Some((tx.producer_id.0, tx.producer_epoch));
        for t in &tx.topics {
            f.topic(&t.name.0, t.partitions.iter().copied());
        }
    }
}

pub(super) fn add_partitions_response(r: &AddPartitionsToTxnResponse, f: &mut Facts) {
    f.error(r.error_code);
    for t in &r.results_by_topic_v3_and_below {
        for p in &t.results_by_partition {
            f.result(&t.name.0, p.partition_index, p.partition_error_code, None);
        }
    }
    for tx in &r.results_by_transaction {
        for t in &tx.topic_results {
            for p in &t.results_by_partition {
                f.result(&t.name.0, p.partition_index, p.partition_error_code, None);
            }
        }
    }
}

pub(super) fn add_offsets_request(r: &AddOffsetsToTxnRequest, f: &mut Facts) {
    f.txn(&r.transactional_id.0);
    f.group(&r.group_id.0);
    f.producer = Some((r.producer_id.0, r.producer_epoch));
}

pub(super) fn add_offsets_response(r: &AddOffsetsToTxnResponse, f: &mut Facts) {
    f.error(r.error_code);
}

pub(super) fn end_request(r: &EndTxnRequest, f: &mut Facts) {
    f.txn(&r.transactional_id.0);
    f.producer = Some((r.producer_id.0, r.producer_epoch));
    f.about(if r.committed { "commit" } else { "abort" });
}

pub(super) fn end_response(r: &EndTxnResponse, f: &mut Facts) {
    f.error(r.error_code);
}

pub(super) fn commit_request(r: &TxnOffsetCommitRequest, f: &mut Facts) {
    f.txn(&r.transactional_id.0);
    f.group(&r.group_id.0);
    f.producer = Some((r.producer_id.0, r.producer_epoch));
    for t in &r.topics {
        f.topic(&t.name.0, t.partitions.iter().map(|p| p.partition_index));
    }
}

pub(super) fn commit_response(r: &TxnOffsetCommitResponse, f: &mut Facts) {
    for t in &r.topics {
        for p in &t.partitions {
            f.result(&t.name.0, p.partition_index, p.error_code, None);
        }
    }
}

pub(super) fn describe_producers_request(r: &DescribeProducersRequest, f: &mut Facts) {
    for t in &r.topics {
        f.topic(&t.name.0, t.partition_indexes.iter().copied());
    }
}

pub(super) fn describe_producers_response(r: &DescribeProducersResponse, f: &mut Facts) {
    let mut producers = 0usize;
    for t in &r.topics {
        for p in &t.partitions {
            f.result(&t.name.0, p.partition_index, p.error_code, None);
            f.say(p.error_message.as_deref());
            producers += p.active_producers.len();
        }
    }
    f.items = n(producers);
}

pub(super) fn describe_request(r: &DescribeTransactionsRequest, f: &mut Facts) {
    for id in &r.transactional_ids {
        f.txn(&id.0);
    }
    f.asked = n(r.transactional_ids.len());
}

pub(super) fn describe_response(r: &DescribeTransactionsResponse, f: &mut Facts) {
    for s in &r.transaction_states {
        f.error(s.error_code);
    }
    f.items = n(r.transaction_states.len());
}

pub(super) fn list_request(r: &ListTransactionsRequest, f: &mut Facts) {
    f.asked = n(r.state_filters.len() + r.producer_id_filters.len());
    f.about("every transaction");
}

pub(super) fn list_response(r: &ListTransactionsResponse, f: &mut Facts) {
    f.error(r.error_code);
    f.items = n(r.transaction_states.len());
}
