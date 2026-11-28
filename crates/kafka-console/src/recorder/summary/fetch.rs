//! Fetch: the topics and partitions asked for, the first offset, how long it was willing to wait, and what came back (the
//! batch headers' counts and the size, never a record).

use kafka_wire::messages::{FetchRequest, FetchResponse};

use super::produce::{IdText, named};
use crate::recorder::facts::{self, Facts, PartResult};

pub(super) fn request(r: &FetchRequest, f: &mut Facts) {
    f.wait_ms = Some(r.max_wait_ms);
    f.isolation = Some(r.isolation_level);
    f.expect(r.topics.len(), 0);
    for t in &r.topics {
        let mut id: IdText = [0; 36];
        f.topic(
            named(&t.topic.0, t.topic_id, &mut id),
            t.partitions.iter().map(|p| p.partition),
        );
        if f.from_offset.is_none() {
            f.from_offset = t.partitions.first().map(|p| p.fetch_offset);
        }
    }
}

pub(super) fn response(r: &FetchResponse, f: &mut Facts) {
    f.error(r.error_code);
    f.expect(0, r.responses.iter().map(|t| t.partitions.len()).sum());
    for t in &r.responses {
        // Named once per topic, and only for the results that are kept: their name is the request's, shared.
        let mut id: IdText = [0; 36];
        let name = named(&t.topic.0, t.topic_id, &mut id);
        for p in &t.partitions {
            let s = p
                .records
                .as_ref()
                .map(|b| facts::batches(b))
                .unwrap_or_default();
            f.records = f.records.saturating_add(s.records);
            f.batches = f.batches.saturating_add(s.batches);
            f.bytes = f.bytes.saturating_add(s.bytes);
            f.part(p.error_code, |f| PartResult {
                topic: f.shared(name),
                partition: p.partition_index,
                code: p.error_code,
                offset: (p.error_code == 0).then_some(p.high_watermark),
                records: s.records,
                bytes: s.bytes,
            });
        }
    }
}
