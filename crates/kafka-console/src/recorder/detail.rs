//! One request in full, for the drawer, and the story of a fenced producer.

use super::errors;
use super::row::Row;
use super::view::wire;
use super::{Recorder, ring};
use crate::model::{WireDetail, WirePart, WireRow};

impl Recorder {
    /// One request in full, if it is still kept. `story` is filled for a row that carries a transactional id or a producer.
    #[must_use]
    pub fn get(&self, id: u64) -> Option<WireDetail> {
        let inner = self.lock();
        let row = inner.ring.get(id)?;
        let f = &*row.facts;
        let spread = (row.count > 1).then(|| {
            let (lo, hi) = row
                .tooks
                .iter()
                .fold((u32::MAX, 0), |(lo, hi), t| (lo.min(*t), hi.max(*t)));
            (hi, lo, row.median())
        });
        let (words, plain, pass) = errors::of(row.code).map_or_else(Default::default, |e| {
            (e.words.to_owned(), e.plain.to_owned(), e.pass.to_owned())
        });
        Some(WireDetail {
            row: wire(row),
            principal: row.who.principal.to_string(),
            software: row.who.software.as_ref().map(|s| match &row.who.version {
                Some(v) => format!("{s} {v}"),
                None => s.to_string(),
            }),
            peer: row.who.peer.as_ref().map(ToString::to_string),
            correlation: row.correlation,
            conn: row.conn,
            request_bytes: row.request_bytes,
            response_bytes: row.response_bytes,
            acks: f.acks,
            wait_ms: f.wait_ms,
            isolation: f.isolation,
            from_offset: f.from_offset,
            producer: f.producer,
            txn_id: f.txn_id.as_ref().map(ToString::to_string),
            member: f.member.as_ref().map(ToString::to_string),
            generation: f.generation,
            reason: f.reason.as_ref().map(ToString::to_string),
            what: f.what.as_ref().map(ToString::to_string),
            note: f.note.as_ref().map(ToString::to_string),
            record_note: f.record_note.as_ref().map(ToString::to_string),
            records: f.records,
            batches: f.batches,
            bytes: f.bytes,
            items: f.items,
            parts: f
                .results
                .iter()
                .map(|r| WirePart {
                    topic: r.topic.to_string(),
                    partition: r.partition,
                    code: r.code,
                    code_name: errors::name(r.code),
                    offset: r.offset,
                    records: r.records,
                    bytes: r.bytes,
                })
                .collect(),
            parts_more: f.results_more,
            topics: f
                .topics
                .iter()
                .map(|t| (t.name.to_string(), t.partitions.clone()))
                .collect(),
            words,
            plain,
            pass,
            took_spread: spread,
            story: story(&inner.ring, row),
        })
    }
}

/// The requests of the same transactional id or producer that came before this one, oldest first (at most six): how it came
/// to this. InitProducerId rows say the epoch each time the producer was given one.
fn story(ring: &ring::Ring, row: &Row) -> Vec<WireRow> {
    let key = |r: &Row| (r.facts.txn_id.clone(), r.facts.producer.map(|p| p.0));
    let (txn, pid) = key(row);
    if txn.is_none() && pid.is_none() {
        return Vec::new();
    }
    let mut out: Vec<WireRow> = ring
        .iter()
        .filter(|r| r.id < row.id)
        .filter(|r| {
            let (t, p) = key(r);
            (txn.is_some() && t == txn) || (pid.is_some() && p == pid)
        })
        .filter(|r| {
            matches!(
                r.api,
                kafka_wire::messages::ApiKey::InitProducerId
                    | kafka_wire::messages::ApiKey::EndTxn
                    | kafka_wire::messages::ApiKey::AddPartitionsToTxn
            )
        })
        .map(wire)
        .collect();
    let skip = out.len().saturating_sub(6);
    out.drain(..skip);
    out
}
