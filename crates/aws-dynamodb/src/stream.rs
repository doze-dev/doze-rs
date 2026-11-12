//! A table's stream: a record for every change of an item, kept under the stream's own id so that enabling, disabling and enabling again
//! is a new stream. Consumers (a Lambda event source mapping) read records after a sequence number and wait for the notification the
//! service publishes when it appends.

use aws_core::time::format_iso8601;
use aws_expr::{Item, item_size, item_to_json};
use aws_wire::ApiError;
use doze_store::record;
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use crate::core::Core;
use crate::disk::Flow;
use crate::keys::{self, Space};
use crate::meta::Table;
use crate::stage::key_of;

/// How long records stay readable.
const RETENTION_MS: i64 = 24 * 60 * 60 * 1000;
const RECORD_VERSION: u8 = 1;

/// What changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    /// An item was created.
    Insert,
    /// An item was changed.
    Modify,
    /// An item was deleted.
    Remove,
}

impl Event {
    const fn name(self) -> &'static str {
        match self {
            Self::Insert => "INSERT",
            Self::Modify => "MODIFY",
            Self::Remove => "REMOVE",
        }
    }
}

/// One stream record, with the images its stream's view type asks for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// The sequence number: increasing within a stream.
    pub seq: u64,
    /// When the change happened, in milliseconds since the epoch.
    pub time_ms: i64,
    /// What changed.
    pub event: Event,
    /// The key attributes of the item.
    pub keys: Item,
    /// The item before, when the view type has it and there was one.
    pub old: Option<Item>,
    /// The item after, when the view type has it and there is one.
    pub new: Option<Item>,
    /// Removed by the time-to-live sweep rather than by a caller.
    pub by_ttl: bool,
    /// The view type of the stream the record is in.
    pub view: String,
}

impl Record {
    /// The sequence number as the text AWS gives (up to 40 digits).
    #[must_use]
    pub fn sequence_number(&self) -> String {
        format!("{:021}", self.seq)
    }

    /// The record as Lambda delivers it, and as `GetRecords` shows it.
    #[must_use]
    pub fn to_event(&self, region: &str) -> Json {
        let mut inner = serde_json::Map::new();
        inner.insert(
            "ApproximateCreationDateTime".into(),
            json!(self.time_ms / 1000),
        );
        inner.insert("Keys".into(), item_to_json(&self.keys));
        if let Some(n) = &self.new {
            inner.insert("NewImage".into(), item_to_json(n));
        }
        if let Some(o) = &self.old {
            inner.insert("OldImage".into(), item_to_json(o));
        }
        inner.insert("SequenceNumber".into(), json!(self.sequence_number()));
        let size = item_size(&self.keys)
            + self.new.as_ref().map_or(0, item_size)
            + self.old.as_ref().map_or(0, item_size);
        inner.insert("SizeBytes".into(), json!(size));
        inner.insert("StreamViewType".into(), json!(self.view));
        let mut out = json!({
            "eventID": event_id(self),
            "eventName": self.event.name(),
            "eventVersion": "1.1",
            "eventSource": "aws:dynamodb",
            "awsRegion": region,
            "dynamodb": Json::Object(inner),
        });
        if self.by_ttl {
            crate::req::set(
                &mut out,
                "userIdentity",
                json!({"type": "Service", "principalId": "dynamodb.amazonaws.com"}),
            );
        }
        out
    }
}

/// A stable, unique-looking event id from the sequence number.
fn event_id(r: &Record) -> String {
    format!(
        "{:032x}",
        u128::from(r.seq) * 0x9E37_79B9_7F4A_7C15 + u128::from(r.time_ms.unsigned_abs())
    )
}

/// What a read of a stream returns.
#[derive(Clone, Debug)]
pub struct Batch {
    /// The records after the sequence number asked for, in order.
    pub records: Vec<Record>,
    /// The table's name.
    pub table: String,
    /// Whether the stream is still the table's current one and is enabled.
    pub open: bool,
}

/// `arn:aws:dynamodb:<region>:<account>:table/<name>/stream/<label>`.
pub(crate) fn stream_arn(region: &str, account: &str, table: &str, label: &str) -> String {
    format!("arn:aws:dynamodb:{region}:{account}:table/{table}/stream/{label}")
}

/// The label of a stream made at `ms`: `2026-11-12T10:20:30.456`.
pub(crate) fn label_of(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let iso = format_iso8601(secs);
    let head = iso.get(..19).unwrap_or(&iso);
    format!("{head}.{:03}", ms.rem_euclid(1000))
}

fn stream_key(id: &[u8; 16], seq: u64) -> Vec<u8> {
    let mut k = id.to_vec();
    k.extend_from_slice(&seq.to_be_bytes());
    k
}

impl Core {
    /// The last sequence number of a stream, found on disk the first time and then kept.
    fn last_seq(&mut self, id: &[u8; 16]) -> Result<u64, ApiError> {
        if let Some(n) = self.seqs.get(id) {
            return Ok(*n);
        }
        let mut last = 0;
        self.disk
            .scan(Space::Stream, id, &keys::prefix_end(id), true, |k, _| {
                last = k
                    .get(16..24)
                    .and_then(|b| <[u8; 8]>::try_from(b).ok())
                    .map_or(0, u64::from_be_bytes);
                Flow::Stop
            })
            .map_err(|e| ApiError::internal(e.to_string()))?;
        self.seqs.insert(*id, last);
        Ok(last)
    }

    /// Appends the record for a change, if the table has a stream. Old records past their retention are removed.
    pub(crate) fn append_stream(
        &mut self,
        t: &Table,
        now_ms: i64,
        old: Option<&Item>,
        new: Option<&Item>,
        by_ttl: bool,
    ) -> Result<(), ApiError> {
        let Some(s) = t.stream.as_ref().filter(|s| s.enabled) else {
            return Ok(());
        };
        let seq = self.last_seq(&s.id)? + 1;
        self.seqs.insert(s.id, seq);
        let event = match (old, new) {
            (None, _) => Event::Insert,
            (Some(_), Some(_)) => Event::Modify,
            (Some(_), None) => Event::Remove,
        };
        let basis = new.or(old).cloned().unwrap_or_default();
        let pick = |wanted: bool, i: Option<&Item>| i.filter(|_| wanted).cloned();
        let (with_new, with_old) = match s.view.as_str() {
            "NEW_IMAGE" => (true, false),
            "OLD_IMAGE" => (false, true),
            "NEW_AND_OLD_IMAGES" => (true, true),
            _ => (false, false),
        };
        let rec = Record {
            seq,
            time_ms: now_ms,
            event,
            keys: key_of(t, &basis),
            old: pick(with_old, old),
            new: pick(with_new, new),
            by_ttl,
            view: s.view.clone(),
        };
        let bytes = record::encode("stream record", RECORD_VERSION, &rec)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        self.pending
            .put(Space::Stream, stream_key(&s.id, seq), bytes);
        self.appended = true;
        self.trim_stream(&s.id, now_ms)
    }

    /// Removes the records older than the retention, a few at a time.
    fn trim_stream(&mut self, id: &[u8; 16], now_ms: i64) -> Result<(), ApiError> {
        let mut stale = Vec::new();
        self.disk
            .scan(Space::Stream, id, &keys::prefix_end(id), false, |k, v| {
                let old = record::decode::<Record>("stream record", RECORD_VERSION, v)
                    .is_ok_and(|r| r.time_ms < now_ms - RETENTION_MS);
                if old && stale.len() < 100 {
                    stale.push(k.to_vec());
                    Flow::Continue
                } else {
                    Flow::Stop
                }
            })
            .map_err(|e| ApiError::internal(e.to_string()))?;
        for k in stale {
            self.pending.del(Space::Stream, k);
        }
        Ok(())
    }

    /// The records of the stream `arn` after sequence number `after`, at most `limit`.
    pub(crate) fn read_stream(&self, arn: &str, after: u64, limit: usize) -> Result<Batch, String> {
        let (name, label) = arn
            .split_once(":table/")
            .and_then(|(_, rest)| rest.split_once("/stream/"))
            .ok_or_else(|| format!("{arn} is not a stream ARN"))?;
        let t = self
            .tables
            .get(name)
            .ok_or_else(|| format!("no table {name}"))?;
        let s = t
            .stream
            .as_ref()
            .filter(|s| s.label == label)
            .ok_or_else(|| format!("{arn} is not the current stream of {name}"))?;
        let mut records = Vec::new();
        self.disk
            .scan(
                Space::Stream,
                &stream_key(&s.id, after.saturating_add(1)),
                &keys::prefix_end(&s.id),
                false,
                |_, v| {
                    if let Ok(r) = record::decode::<Record>("stream record", RECORD_VERSION, v) {
                        records.push(r);
                    }
                    if records.len() >= limit {
                        Flow::Stop
                    } else {
                        Flow::Continue
                    }
                },
            )
            .map_err(|e| e.to_string())?;
        Ok(Batch {
            records,
            table: name.to_owned(),
            open: s.enabled,
        })
    }
}
