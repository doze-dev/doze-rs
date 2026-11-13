//! Writing records: `PutRecord`, `PutRecords`, and the expiry of records past the stream's retention.

use aws_wire::ApiError;
use doze_store::record;
use serde_json::{Value, json};

use crate::core::Core;
use crate::model::{MAX_HASH, Record, SEQ_SHIFT, hash_of, row_key};
use crate::req::{Put, Req, invalid};

/// Records one `PutRecords` may carry.
const MAX_BATCH: usize = 500;
/// The bytes one `PutRecords` may carry.
const MAX_BATCH_BYTES: usize = 10 * 1024 * 1024;
/// A stream's expired records are removed at most this often.
const TRIM_EVERY_MS: i64 = 60_000;

/// One record to append: its partition key, the hash it routes by, and its bytes.
struct Entry {
    key: String,
    hash: u128,
    data: Vec<u8>,
}

impl Core {
    pub(crate) fn put_record(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let max = self.stream(r)?.max_record_kib * 1024;
        let entry = entry_of(&r.v, max)?;
        let name = r.stream_name().unwrap_or_default();
        let (shard, seq) = self.append(&name, entry)?;
        let mut out =
            json!({"ShardId": crate::model::shard_name(shard), "SequenceNumber": seq.to_string()});
        if self.stream(r)?.kms_key.is_some() {
            out.put("EncryptionType", json!("KMS"));
        }
        Ok(out)
    }

    pub(crate) fn put_records(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let max = self.stream(r)?.max_record_kib * 1024;
        let items =
            r.v.get("Records")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
        if items.len() > MAX_BATCH {
            return Err(invalid(format!(
                "Too many records in the request: {} (the most is {MAX_BATCH}).",
                items.len()
            )));
        }
        let name = r.stream_name().unwrap_or_default();
        let (mut results, mut failed, mut total) = (Vec::new(), 0, 0usize);
        for item in items {
            let entry = entry_of(item, max);
            total += entry.as_ref().map_or(0, |e| e.data.len() + e.key.len());
            if total > MAX_BATCH_BYTES {
                return Err(invalid(
                    "The total size of the records in the request is over 10 MiB.",
                ));
            }
            match entry.and_then(|e| self.append(&name, e)) {
                Ok((shard, seq)) => results.push(json!({
                    "ShardId": crate::model::shard_name(shard),
                    "SequenceNumber": seq.to_string(),
                })),
                Err(e) => {
                    failed += 1;
                    results.push(json!({"ErrorCode": e.code, "ErrorMessage": e.message}));
                }
            }
        }
        let mut out = json!({"FailedRecordCount": failed, "Records": results});
        if self.stream(r)?.kms_key.is_some() {
            out.put("EncryptionType", json!("KMS"));
        }
        Ok(out)
    }

    /// Appends a record to the shard its hash routes to. Returns the shard and the sequence number.
    fn append(&mut self, name: &str, entry: Entry) -> Result<(u32, u128), ApiError> {
        self.trim(name);
        let now = self.now_ms();
        let floor = u128::try_from(now).unwrap_or(0) << SEQ_SHIFT;
        let stream = self
            .streams
            .get_mut(name)
            .ok_or_else(|| ApiError::internal("the stream went away"))?;
        let no = stream.no;
        let shard = stream
            .shards
            .iter_mut()
            .find(|s| s.holds(entry.hash))
            .ok_or_else(|| invalid("No open shard holds that hash key."))?;
        // Never below the number the shard starts at (a child opened in this very millisecond starts a little above the floor).
        let seq = (shard.last_seq + 1).max(floor).max(shard.first_seq);
        shard.last_seq = seq;
        let id = shard.id;
        let rec = Record {
            arrived_ms: now,
            partition_key: entry.key,
            data: entry.data,
        };
        let bytes =
            record::encode("record", 1, &rec).map_err(|e| ApiError::internal(e.to_string()))?;
        self.rows.put(row_key(no, id, seq), bytes);
        self.appended = true;
        Ok((id, seq))
    }

    /// Removes the records older than the stream's retention (lazily: when the stream is written, and not more than once a minute).
    pub(crate) fn trim(&mut self, name: &str) {
        let now = self.now_ms();
        if self
            .trimmed
            .get(name)
            .is_some_and(|at| now - at < TRIM_EVERY_MS)
        {
            return;
        }
        self.trimmed.insert(name.to_owned(), now);
        let Some(stream) = self.streams.get(name) else {
            return;
        };
        let cutoff_ms = now - stream.retention_hours * 3_600_000;
        let cutoff = u128::try_from(cutoff_ms).unwrap_or(0) << SEQ_SHIFT;
        if cutoff == 0 {
            return;
        }
        let ranges: Vec<(String, String)> = stream
            .shards
            .iter()
            .map(|s| {
                (
                    row_key(stream.no, s.id, 0),
                    row_key(stream.no, s.id, cutoff),
                )
            })
            .collect();
        for (from, to) in ranges {
            self.rows.delete_range(&from, &to);
        }
        // A closed shard whose last record is older than the retention holds nothing and is no longer listed.
        if let Some(stream) = self.streams.get(name)
            && stream
                .shards
                .iter()
                .any(|s| s.ended_seq.is_some_and(|e| e < cutoff))
            && let Some(stream) = self.streams.get_mut(name)
        {
            stream
                .shards
                .retain(|s| s.ended_seq.is_none_or(|e| e >= cutoff));
        }
    }
}

/// A record from a request member (`PutRecord`'s own members, or one entry of `PutRecords`).
fn entry_of(v: &Value, max_bytes: i64) -> Result<Entry, ApiError> {
    let key = v
        .get("PartitionKey")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let data = v
        .get("Data")
        .and_then(Value::as_str)
        .and_then(aws_core::base64::decode)
        .unwrap_or_default();
    if i64::try_from(data.len() + key.len()).unwrap_or(i64::MAX) > max_bytes {
        // (memory)
        return Err(ApiError::modeled(
            "ValidationException",
            format!(
                "1 validation error detected: Value at 'data' failed to satisfy constraint: Member must have length less than or equal to {max_bytes}"
            ),
        ));
    }
    let hash = match v.get("ExplicitHashKey").and_then(Value::as_str) {
        Some(text) => match text.parse::<u128>() {
            Ok(h) => h,
            // (memory) a number past 2^128 - 1 matches the model's pattern and is not a key.
            Err(_) => {
                return Err(invalid(format!(
                    "Invalid ExplicitHashKey. ExplicitHashKey must be in the range: [0, {MAX_HASH}]. Specified value was {text}"
                )));
            }
        },
        None => hash_of(key),
    };
    Ok(Entry {
        key: key.to_owned(),
        hash,
        data,
    })
}
