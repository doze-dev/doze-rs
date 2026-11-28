//! Produce: acks, the transactional id, topics and partitions, and the batch headers' record counts.

use kafka_wire::messages::{ProduceRequest, ProduceResponse};

use crate::recorder::clip;
use crate::recorder::facts::{self, Facts, PartResult};

pub(super) fn request(r: &ProduceRequest, f: &mut Facts) {
    f.acks = Some(r.acks);
    if let Some(t) = &r.transactional_id {
        f.txn(&t.0);
    }
    f.expect(r.topic_data.len(), 0);
    for t in &r.topic_data {
        let mut id: IdText = [0; 36];
        f.topic(
            named(&t.name.0, t.topic_id, &mut id),
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
    let results = r.responses.iter().map(|t| t.partition_responses.len());
    f.expect(0, results.sum());
    for t in &r.responses {
        // Named once per topic, and only for the results that are kept: their name is the request's, shared.
        let mut id: IdText = [0; 36];
        let name = named(&t.name.0, t.topic_id, &mut id);
        for p in &t.partition_responses {
            f.part(p.error_code, |f| PartResult {
                topic: f.shared(name),
                partition: p.index,
                code: p.error_code,
                offset: (p.error_code == 0).then_some(p.base_offset),
                records: 0,
                bytes: 0,
            });
            f.say(p.error_message.as_deref());
            if let Some(e) = p.record_errors.first() {
                f.record_note = e.batch_index_error_message.as_deref().and_then(clip::text);
            }
        }
    }
}

/// Room on the stack for a topic id written out as text (36 bytes, hyphenated).
pub(super) type IdText = [u8; 36];

/// A topic as a request or answer names it: by name, or from v13 (KIP-516) by id, as Fetch's are. An id is written into
/// `id`, so neither way allocates.
pub(super) fn named<'a>(name: &'a str, topic_id: kafka_wire::Uuid, id: &'a mut IdText) -> &'a str {
    if name.is_empty() {
        topic_id.hyphenated().encode_lower(id)
    } else {
        name
    }
}
