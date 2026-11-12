//! One record, completely: its key, value, headers, time, codec, producer and the transaction it belongs to.

use super::decode::{TEXT_CAP, shown};
use super::meta;
use super::read;
use super::txns;
use super::walk::{Keep, Raw};
use crate::backend::Backend;
use crate::model::{HeaderRow, Reading, RecordTxn, RecordView};

/// What one record is read with: its key and value in full up to the display limit, headers to 1 KiB a value.
const WHOLE: Keep = Keep {
    key: TEXT_CAP,
    value: TEXT_CAP,
    header: 1024,
};

/// The record at `offset` of `partition` of `topic`. `None` if there is no such topic or partition, the offset is outside the
/// log, or no record is there (compaction or a transaction marker took it).
pub async fn one(b: &Backend, topic: &str, partition: i32, offset: i64) -> Option<RecordView> {
    let m = meta::one(b, topic).await?;
    if !m.partitions.contains(&partition) || offset < 0 {
        return None;
    }
    let chunk = read::once(
        b,
        m.id,
        (partition, offset),
        Reading::Uncommitted,
        (WHOLE, 1),
    )
    .await;
    let raw: Raw = chunk.rows.into_iter().find(|r| r.offset == offset)?;
    let open = txns::ongoing(b).await;
    Some(view(topic, &raw, chunk.lso, &open))
}

fn txn_of(raw: &Raw, open: &[txns::Open]) -> Option<RecordTxn> {
    if !raw.transactional {
        return None;
    }
    let producer = raw.producer.map(|(id, _, _)| id)?;
    let t = open.iter().find(|t| t.producer_id == producer);
    Some(RecordTxn {
        id: t.map(|t| t.id.clone()),
        state: t.map(|t| t.state.clone()),
        since_ms: t.map(|t| t.since_ms),
        timeout_ms: t.map(|t| t.timeout_ms),
    })
}

fn view(topic: &str, raw: &Raw, lso: i64, open: &[txns::Open]) -> RecordView {
    RecordView {
        topic: topic.to_owned(),
        partition: raw.partition,
        offset: raw.offset,
        last_stable: lso,
        key: shown(raw.key.as_deref(), raw.key_len),
        value: shown(raw.value.as_deref(), raw.value_len),
        headers: raw
            .headers
            .iter()
            .map(|(k, v, n)| HeaderRow {
                key: super::decode::clean(&String::from_utf8_lossy(k)),
                value: shown(v.as_deref(), *n),
            })
            .collect(),
        timestamp_ms: raw.timestamp,
        log_append_time: raw.log_append,
        codec: raw.codec.to_owned(),
        producer: raw.producer,
        transactional: raw.transactional,
        txn: txn_of(raw, open),
        standing: raw.standing,
    }
}
