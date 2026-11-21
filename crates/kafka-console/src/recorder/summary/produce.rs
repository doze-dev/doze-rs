//! Produce: acks, the transactional id, topics and partitions, and the batch headers' record counts.

use kafka_wire::messages::{ProduceRequest, ProduceResponse};

use crate::recorder::facts::{self, Facts, PartResult};

pub(super) fn request(r: &ProduceRequest, f: &mut Facts) {
    f.acks = Some(r.acks);
    if let Some(t) = &r.transactional_id {
        f.txn(&t.0);
    }
    for t in &r.topic_data {
        f.topic(
            &named(&t.name.0, t.topic_id),
            t.partition_data.iter().map(|p| p.index),
        );
        for p in &t.partition_data {
            if let Some(records) = &p.records {
                let s = facts::batches(records);
                f.records = f.records.saturating_add(s.records);
                f.batches = f.batches.saturating_add(s.batches);
                f.bytes = f.bytes.saturating_add(s.bytes);
                f.transactional |= s.transactional;
                if f.producer.is_none() {
                    f.producer = s.producer;
                }
            }
        }
    }
}

pub(super) fn response(r: &ProduceResponse, f: &mut Facts) {
    for t in &r.responses {
        for p in &t.partition_responses {
            f.part(PartResult {
                topic: crate::recorder::clip::name(&named(&t.name.0, t.topic_id)),
                partition: p.index,
                code: p.error_code,
                offset: (p.error_code == 0).then_some(p.base_offset),
                records: 0,
                bytes: 0,
            });
            f.say(p.error_message.as_deref());
            if let Some(e) = p.record_errors.first() {
                f.record_note = e
                    .batch_index_error_message
                    .as_deref()
                    .and_then(crate::recorder::clip::text);
            }
        }
    }
}

/// A topic as a request or answer names it: by name, or from v13 (KIP-516) by id, as Fetch's are.
pub(super) fn named(name: &str, id: kafka_wire::Uuid) -> String {
    if name.is_empty() {
        id.to_string()
    } else {
        name.to_owned()
    }
}
