//! Reading: shard iterators, `GetRecords`, `ListShards`.

use aws_wire::ApiError;
use doze_store::record;
use serde_json::{Value, json};

use crate::core::{Core, not_found};
use crate::model::{Record, SEQ_SHIFT, row_key, shard_name, shard_number};
use crate::req::{Put, Req, invalid, secs};
use crate::streams::shard_json;

/// A shard iterator is good for this long (AWS: five minutes).
const ITERATOR_MS: i64 = 300_000;
/// Records one `GetRecords` returns at most (the API's own limit).
const MAX_RECORDS: usize = 10_000;
/// Bytes one `GetRecords` returns at most (AWS: 10 MiB).
const MAX_BYTES: usize = 10 * 1024 * 1024;
/// Rows read from the table at a time.
const CHUNK: usize = 100;

/// Where an iterator points: the stream, the shard, the next sequence number to read, and when it was made.
struct Cursor {
    stream: String,
    shard: u32,
    next: u128,
    issued_ms: i64,
}

fn make_iterator(c: &Cursor) -> String {
    aws_core::base64::encode(
        format!("{}|{}|{}|{}", c.stream, c.shard, c.next, c.issued_ms).as_bytes(),
    )
}

fn read_iterator(text: &str) -> Option<Cursor> {
    let raw = String::from_utf8(aws_core::base64::decode(text)?).ok()?;
    let mut parts = raw.split('|');
    let cursor = Cursor {
        stream: parts.next()?.to_owned(),
        shard: parts.next()?.parse().ok()?,
        next: parts.next()?.parse().ok()?,
        issued_ms: parts.next()?.parse().ok()?,
    };
    parts.next().is_none().then_some(cursor)
}

/// The stream an iterator reads, for the authorizer.
pub(crate) fn stream_of_iterator(text: &str) -> Option<String> {
    read_iterator(text).map(|c| c.stream)
}

impl Core {
    pub(crate) fn get_shard_iterator(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let s = self.stream(r)?;
        let id = r.s("ShardId");
        let shard = shard_number(id).and_then(|n| s.shard(n)).ok_or_else(|| {
            ApiError::modeled(
                "ResourceNotFoundException",
                format!(
                    "Shard {id} in stream {} under account {} does not exist",
                    s.name,
                    r.account()
                ),
            )
        })?;
        let kind = r.s("ShardIteratorType");
        let from = r.opt("StartingSequenceNumber").map(|t| t.parse::<u128>());
        let next = match (kind, from) {
            ("AT_SEQUENCE_NUMBER", Some(Ok(n))) => n,
            ("AFTER_SEQUENCE_NUMBER", Some(Ok(n))) => n.saturating_add(1),
            ("AT_SEQUENCE_NUMBER" | "AFTER_SEQUENCE_NUMBER", Some(Err(_))) => {
                return Err(invalid(
                    "StartingSequenceNumber is not a valid sequence number.",
                ));
            }
            ("TRIM_HORIZON", None) => 0,
            ("LATEST", None) => shard.last_seq.saturating_add(1),
            ("AT_TIMESTAMP", None) => at_timestamp(r)?,
            (kind, from) => {
                // Recorded on AWS (2026-11-09) for AT_SEQUENCE_NUMBER with no number; the other kinds are worded alike (memory).
                let given = if from.is_some() {
                    format!("{kind} and StartingSequenceNumber")
                } else {
                    format!("{kind} and no StartingSequenceNumber")
                };
                return Err(invalid(format!(
                    "Must either specify (1) AT_SEQUENCE_NUMBER or AFTER_SEQUENCE_NUMBER and StartingSequenceNumber or (2) TRIM_HORIZON or LATEST and no StartingSequenceNumber. Request specified {given}."
                )));
            }
        };
        let cursor = Cursor {
            stream: s.name.clone(),
            shard: shard.id,
            next,
            issued_ms: self.now_ms(),
        };
        Ok(json!({"ShardIterator": make_iterator(&cursor)}))
    }

    pub(crate) fn get_records(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let cursor =
            read_iterator(r.s("ShardIterator")).ok_or_else(|| invalid("Invalid ShardIterator."))?;
        let now = self.now_ms();
        if now - cursor.issued_ms > ITERATOR_MS {
            // (memory)
            return Err(ApiError::modeled(
                "ExpiredIteratorException",
                format!(
                    "Iterator expired. The iterator was created at time {} while right now it is {} which is further in the future than the tolerated delay of {ITERATOR_MS} milliseconds.",
                    cursor.issued_ms, now
                ),
            ));
        }
        let (no, tip) = {
            let s = self
                .streams
                .get(&cursor.stream)
                .ok_or_else(|| not_found(&cursor.stream, r.account()))?;
            let sh = s
                .shard(cursor.shard)
                .ok_or_else(|| invalid("Invalid ShardIterator."))?;
            (s.no, sh.last_seq)
        };
        self.trim(&cursor.stream);
        let limit = r
            .int("Limit")
            .map_or(MAX_RECORDS, |n| usize::try_from(n).unwrap_or(MAX_RECORDS));
        let (found, next) = self.read_from(no, &cursor, limit)?;
        let behind = found.last().map_or(0, |(seq, _)| millis_between(*seq, tip));
        let kms = self
            .streams
            .get(&cursor.stream)
            .is_some_and(|s| s.kms_key.is_some());
        let records: Vec<Value> = found
            .iter()
            .map(|(seq, rec)| {
                let mut v = json!({
                    "SequenceNumber": seq.to_string(),
                    "ApproximateArrivalTimestamp": secs(rec.arrived_ms),
                    "Data": aws_core::base64::encode(&rec.data),
                    "PartitionKey": rec.partition_key,
                });
                if kms {
                    v.put("EncryptionType", json!("KMS"));
                }
                v
            })
            .collect();
        let after = Cursor {
            next,
            issued_ms: now,
            ..cursor
        };
        Ok(json!({
            "Records": records,
            "NextShardIterator": make_iterator(&after),
            "MillisBehindLatest": behind,
        }))
    }

    /// The records of a shard from `cursor.next` on: at most `limit` of them and about `MAX_BYTES`. Also the sequence number to
    /// continue from.
    fn read_from(
        &self,
        no: u32,
        cursor: &Cursor,
        limit: usize,
    ) -> Result<(Vec<(u128, Record)>, u128), ApiError> {
        let end = format!("{no:08x}{:08x}", cursor.shard.saturating_add(1));
        let (mut at, mut bytes, mut found) = (cursor.next, 0usize, Vec::new());
        'chunks: while found.len() < limit {
            let want = (limit - found.len()).min(CHUNK);
            let rows = self
                .rows
                .scan(
                    self.db.as_ref(),
                    &row_key(no, cursor.shard, at),
                    &end,
                    want,
                    false,
                )
                .map_err(|e| ApiError::internal(e.to_string()))?;
            let n = rows.len();
            for (key, value) in rows {
                let seq = key
                    .get(16..)
                    .and_then(|h| u128::from_str_radix(h, 16).ok())
                    .ok_or_else(|| ApiError::internal("a record key is unreadable"))?;
                let rec: Record = record::decode("record", 1, &value)
                    .map_err(|e| ApiError::internal(e.to_string()))?;
                bytes += rec.data.len();
                at = seq.saturating_add(1);
                found.push((seq, rec));
                if bytes >= MAX_BYTES {
                    break 'chunks;
                }
            }
            if n < want {
                break;
            }
        }
        Ok((found, at))
    }

    pub(crate) fn list_shards(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (s, token_after) = match r.opt("NextToken") {
            Some(token) => {
                let (name, after) = token
                    .split_once('|')
                    .ok_or_else(|| invalid("Invalid NextToken."))?;
                let s = self
                    .streams
                    .get(name)
                    .ok_or_else(|| not_found(name, r.account()))?;
                (s, shard_number(after))
            }
            None => {
                if r.stream_name().is_none() {
                    return Err(invalid("Either StreamName or NextToken must be specified."));
                }
                (self.stream(r)?, None)
            }
        };
        let after = token_after
            .or_else(|| r.opt("ExclusiveStartShardId").and_then(shard_number))
            .or_else(|| {
                let filter = r.v.get("ShardFilter")?;
                (filter.get("Type")?.as_str()? == "AFTER_SHARD_ID")
                    .then(|| filter.get("ShardId")?.as_str().and_then(shard_number))
                    .flatten()
            });
        let latest = r.v.pointer("/ShardFilter/Type").and_then(Value::as_str) == Some("AT_LATEST");
        let limit = r
            .int("MaxResults")
            .map_or(1000, |n| usize::try_from(n).unwrap_or(1000));
        let rest: Vec<_> = s
            .shards
            .iter()
            .filter(|sh| after.is_none_or(|a| sh.id > a) && (!latest || sh.ended_seq.is_none()))
            .collect();
        let shown: Vec<Value> = rest.iter().take(limit).map(|sh| shard_json(sh)).collect();
        let mut out = json!({ "Shards": shown });
        if let (true, Some(last)) = (rest.len() > limit, rest.get(limit.saturating_sub(1))) {
            out.put(
                "NextToken",
                json!(format!("{}|{}", s.name, shard_name(last.id))),
            );
        }
        Ok(out)
    }
}

/// The sequence number a timestamp stands for.
fn at_timestamp(r: &Req<'_>) -> Result<u128, ApiError> {
    let secs =
        r.v.get("Timestamp")
            .and_then(Value::as_f64)
            // (memory)
            .ok_or_else(|| {
                invalid("Timestamp must be specified when ShardIteratorType is AT_TIMESTAMP.")
            })?;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ms = (secs.max(0.0) * 1000.0) as u128;
    Ok(ms << SEQ_SHIFT)
}

/// Milliseconds between the arrival of sequence number `from` and that of `tip`.
fn millis_between(from: u128, tip: u128) -> i64 {
    let ms = |s: u128| i64::try_from(s >> SEQ_SHIFT).unwrap_or(0);
    (ms(tip) - ms(from)).max(0)
}
