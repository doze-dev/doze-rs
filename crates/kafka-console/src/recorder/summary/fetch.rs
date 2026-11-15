//! Fetch: the topics and partitions asked for, the first offset, how long it was willing to wait, and what came back (the
//! batch headers' counts and the size, never a record).

use kafka_wire::messages::{FetchRequest, FetchResponse};

use crate::recorder::facts::{self, Facts, PartResult};

pub(super) fn request(r: &FetchRequest, f: &mut Facts) {
    f.wait_ms = Some(r.max_wait_ms);
    f.isolation = Some(r.isolation_level);
    for t in &r.topics {
        let name = if t.topic.0.is_empty() {
            t.topic_id.to_string()
        } else {
            t.topic.0.to_string()
        };
        f.topic(&name, t.partitions.iter().map(|p| p.partition));
        if f.from_offset.is_none() {
            f.from_offset = t.partitions.first().map(|p| p.fetch_offset);
        }
    }
}

pub(super) fn response(r: &FetchResponse, f: &mut Facts) {
    f.error(r.error_code);
    for t in &r.responses {
        let name = if t.topic.0.is_empty() {
            t.topic_id.to_string()
        } else {
            t.topic.0.to_string()
        };
        for p in &t.partitions {
            let s = p
                .records
                .as_ref()
                .map(|b| facts::batches(b))
                .unwrap_or_default();
            f.records = f.records.saturating_add(s.records);
            f.batches = f.batches.saturating_add(s.batches);
            f.bytes = f.bytes.saturating_add(s.bytes);
            f.part(PartResult {
                topic: crate::recorder::clip::name(&name),
                partition: p.partition_index,
                code: p.error_code,
                offset: (p.error_code == 0).then_some(p.high_watermark),
                records: s.records,
                bytes: s.bytes,
            });
        }
    }
}
