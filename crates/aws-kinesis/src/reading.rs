//! Reading: shard iterators, `GetRecords`, `ListShards`.

use aws_wire::ApiError;
use doze_store::record;
use serde_json::{Value, json};

use crate::core::{Core, not_found, shard_missing};
use crate::model::{Record, Stream, ms_seq, row_key, seq_ms, shard_name, shard_number};
use crate::req::{Put, Req, invalid, secs};
use crate::streams::{child_shards_json, shard_json};

/// A shard iterator is good for this long (AWS: five minutes).
const ITERATOR_MS: i64 = 300_000;
/// Records one `GetRecords` returns at most (the API's own limit).
const MAX_RECORDS: usize = 10_000;
/// Bytes one `GetRecords` returns at most (AWS: 10 MiB).
const MAX_BYTES: usize = 10 * 1024 * 1024;
/// Rows read from the table at a time.
const CHUNK: usize = 100;

/// What a read of a shard found: the records with their sequence numbers, where to read on from, and whether nothing more follows.
type Read = (Vec<(u128, Record)>, u128, bool);

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
        let shard = shard_number(id)
            .and_then(|n| s.shard(n))
            .ok_or_else(|| shard_missing(id, &s.name, r.account()))?;
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
        let (found, next, exhausted) = self.read_from(no, &cursor, limit)?;
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
        // A closed shard that has been read to its last record has ended: no iterator to continue with, and the children to read
        // instead. (Recorded on AWS: a split closes the parent at once. The answer at its end is memory: the model says "If set to
        // null, the shard has been closed and the requested iterator does not return any more data", and `ChildShards` "only when
        // the end of the current shard is reached".)
        if let Some(children) = self.children_at_end(&cursor, exhausted) {
            return Ok(json!({
                "Records": records,
                "MillisBehindLatest": 0,
                "ChildShards": children,
            }));
        }
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

    /// The children a closed shard hands over to, if the read has reached its end.
    fn children_at_end(&self, cursor: &Cursor, exhausted: bool) -> Option<Vec<Value>> {
        let s = self.streams.get(&cursor.stream)?;
        let closed = s.shard(cursor.shard)?.ended_seq.is_some();
        (exhausted && closed).then(|| child_shards_json(s, cursor.shard))
    }

    /// The records of a shard from `cursor.next` on: at most `limit` of them and about `MAX_BYTES`. Also the sequence number to
    /// continue from, and whether the shard has nothing more after what was read.
    fn read_from(&self, no: u32, cursor: &Cursor, limit: usize) -> Result<Read, ApiError> {
        let end = format!("{no:08x}{:08x}", cursor.shard.saturating_add(1));
        let (mut at, mut bytes, mut found) = (cursor.next, 0usize, Vec::new());
        let mut exhausted = false;
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
                exhausted = true;
                break;
            }
        }
        Ok((found, at, exhausted))
    }

    pub(crate) fn list_shards(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        // A token names the stream and where the last page stopped, and carries the filter the first page was asked with.
        let (s, token_after, token_filter) = match r.opt("NextToken") {
            Some(token) => {
                let mut parts = token.splitn(3, '|');
                let (name, after, filter) = (parts.next(), parts.next(), parts.next());
                let (Some(name), Some(after)) = (name, after) else {
                    return Err(invalid("Invalid NextToken."));
                };
                let s = self
                    .streams
                    .get(name)
                    .ok_or_else(|| not_found(name, r.account()))?;
                let filter = filter
                    .map(|f| Filter::from_token(f).ok_or_else(|| invalid("Invalid NextToken.")))
                    .transpose()?;
                (s, shard_number(after), filter)
            }
            None => {
                if r.stream_name().is_none() {
                    return Err(invalid("Either StreamName or NextToken must be specified."));
                }
                (self.stream(r)?, None, None)
            }
        };
        let (asked, filter_after) = Filter::of(r)?;
        let filter = token_filter.unwrap_or(asked);
        let after = token_after
            .or_else(|| r.opt("ExclusiveStartShardId").and_then(shard_number))
            .or(filter_after);
        let limit = r
            .int("MaxResults")
            .map_or(1000, |n| usize::try_from(n).unwrap_or(1000));
        let now = self.now_ms();
        let rest: Vec<_> = s
            .shards
            .iter()
            .filter(|sh| after.is_none_or(|a| sh.id > a) && filter.shows(s, sh, now))
            .collect();
        let shown: Vec<Value> = rest.iter().take(limit).map(|sh| shard_json(sh)).collect();
        let mut out = json!({ "Shards": shown });
        if let (true, Some(last)) = (rest.len() > limit, rest.get(limit.saturating_sub(1))) {
            out.put(
                "NextToken",
                json!(format!(
                    "{}|{}|{}",
                    s.name,
                    shard_name(last.id),
                    filter.token()
                )),
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
    Ok(ms_seq(millis_of(secs)))
}

/// A timestamp in seconds since the epoch, in milliseconds.
#[allow(clippy::cast_possible_truncation)]
fn millis_of(secs: f64) -> i64 {
    (secs.max(0.0) * 1000.0) as i64
}

/// Milliseconds between the arrival of sequence number `from` and that of `tip`.
fn millis_between(from: u128, tip: u128) -> i64 {
    (seq_ms(tip) - seq_ms(from)).max(0)
}

/// Which shards a `ListShards` shows (`ShardFilter`).
#[derive(Clone, Copy, Debug)]
enum Filter {
    /// Every shard within the retention period: the default, and `FROM_TRIM_HORIZON`.
    All,
    AtTrimHorizon,
    AtLatest,
    AtTimestamp(i64),
    FromTimestamp(i64),
}

impl Filter {
    /// The filter a request asks for, and the shard `AFTER_SHARD_ID` names.
    fn of(r: &Req<'_>) -> Result<(Self, Option<u32>), ApiError> {
        let Some(f) = r.v.get("ShardFilter") else {
            return Ok((Self::All, None));
        };
        let ts = || {
            f.get("Timestamp")
                .and_then(Value::as_f64)
                .map(millis_of)
                // (memory)
                .ok_or_else(|| {
                    invalid("Timestamp must be specified when the ShardFilter type is AT_TIMESTAMP or FROM_TIMESTAMP.")
                })
        };
        match f.get("Type").and_then(Value::as_str).unwrap_or_default() {
            "AFTER_SHARD_ID" => {
                let id = f
                    .get("ShardId")
                    .and_then(Value::as_str)
                    .and_then(shard_number);
                // (memory)
                id.map(|n| (Self::All, Some(n))).ok_or_else(|| {
                    invalid(
                        "ShardId must be specified when the ShardFilter type is AFTER_SHARD_ID.",
                    )
                })
            }
            "AT_TRIM_HORIZON" => Ok((Self::AtTrimHorizon, None)),
            "AT_LATEST" => Ok((Self::AtLatest, None)),
            "AT_TIMESTAMP" => Ok((Self::AtTimestamp(ts()?), None)),
            "FROM_TIMESTAMP" => Ok((Self::FromTimestamp(ts()?), None)),
            _ => Ok((Self::All, None)),
        }
    }

    /// The filter written into a `NextToken`, so the next page is cut the same way.
    fn token(self) -> String {
        match self {
            Self::All => "ALL".to_owned(),
            Self::AtTrimHorizon => "AT_TRIM_HORIZON".to_owned(),
            Self::AtLatest => "AT_LATEST".to_owned(),
            Self::AtTimestamp(ms) => format!("AT_TIMESTAMP:{ms}"),
            Self::FromTimestamp(ms) => format!("FROM_TIMESTAMP:{ms}"),
        }
    }

    fn from_token(text: &str) -> Option<Self> {
        let (kind, ms) = match text.split_once(':') {
            Some((k, v)) => (k, Some(v.parse::<i64>().ok()?)),
            None => (text, None),
        };
        Some(match (kind, ms) {
            ("ALL", None) => Self::All,
            ("AT_TRIM_HORIZON", None) => Self::AtTrimHorizon,
            ("AT_LATEST", None) => Self::AtLatest,
            ("AT_TIMESTAMP", Some(ms)) => Self::AtTimestamp(ms),
            ("FROM_TIMESTAMP", Some(ms)) => Self::FromTimestamp(ms),
            _ => return None,
        })
    }

    /// Whether the shard is shown. A shard that was closed ended at `closed_ms`; one still open has no end.
    fn shows(self, s: &Stream, shard: &crate::model::Shard, now_ms: i64) -> bool {
        let open_at =
            |t: i64| shard.opened_ms() <= t && shard.closed_ms().is_none_or(|end| end >= t);
        match self {
            Self::All => true,
            Self::AtLatest => shard.ended_seq.is_none(),
            Self::AtTimestamp(t) => open_at(t),
            // Shards that ended before the timestamp are dropped; every other is open at it or begins after it.
            Self::FromTimestamp(t) => shard.closed_ms().is_none_or(|end| end >= t),
            Self::AtTrimHorizon => {
                // The trim horizon is the start of what is kept: the retention period back from now, or the first shard's opening
                // if the stream is younger than that.
                let oldest = s.shards.iter().map(crate::model::Shard::opened_ms).min();
                let horizon =
                    (now_ms - s.retention_hours * 3_600_000).max(oldest.unwrap_or(now_ms));
                open_at(horizon)
            }
        }
    }
}
