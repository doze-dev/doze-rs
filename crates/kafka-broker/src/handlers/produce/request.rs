//! What fails a whole Produce request before any partition is looked at.

use kafka_wire::ResponseError;
use kafka_wire::batch::{Batch, ProduceError, Refusal};
use kafka_wire::messages::produce_response::TopicProduceResponse;
use kafka_wire::messages::{ProduceRequest, ProduceResponse};

use super::{Refused, failed};

/// What fails the whole request before any partition is looked at: a \`required acks\` that is not -1, 0 or 1; and
/// Kafka's look at every partition's first batch, which fails a request it cannot take apart there and one that has
/// a transactional batch and no transactional id, or one the caller may not WRITE (`may_write_txn`; Kafka asks that before
/// anything else, `KafkaApis.handleProduceRequest`).
pub(super) fn request_refusal(
    req: &ProduceRequest,
    may_write_txn: impl Fn(&str) -> bool,
) -> Option<Refused> {
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
    // (Asked only when a batch is transactional: a request that is not has no use for the id's ACLs.)
    let refused = transactional
        && !req
            .transactional_id
            .as_ref()
            .is_some_and(|id| may_write_txn(id.0.as_str()));
    refused.then_some((ResponseError::TransactionalIdAuthorizationFailed, None))
}

fn code_of(e: &ProduceError) -> ResponseError {
    match e.refusal() {
        Refusal::CorruptMessage => ResponseError::CorruptMessage,
        _ => ResponseError::InvalidRecord,
    }
}

pub(super) fn refused_everywhere(
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
