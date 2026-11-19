//! A Produce v0 to v2 message set, made one v2 batch (Kafka's `Log.analyzeAndValidateRecords`, then `LogValidator`).
//!
//! Kafka holds these requests to no shape at the request (its `ProduceRequest.validateRecords` looks only from v3), so the
//! set may hold messages of either old format, a v2 batch, or several entries. First every entry is looked at as it is:
//! over `max.message.bytes` is MESSAGE_TOO_LARGE, a checksum that is not its contents' CORRUPT_MESSAGE. Then, from Kafka
//! 2.0 (`strict`), a compressed set or one led by a v2 batch must be one entry, and every entry must have the first's
//! format (INVALID_RECORD); Kafka 1.1 converts whatever came. A set that is one v2 batch is taken as it is, as from v3.
//! Anything else is read message by message (the inner messages of a wrapper through its codec: each must have the
//! wrapper's format and no codec of its own (INVALID_RECORD), and a valid checksum (CORRUPT_MESSAGE)) into one batch.

use bytes::Bytes;

use super::codec;
use super::message::{self, Entry, Message, Unreadable};
use super::records::{self, Unread};
use super::{Rec, v2_batch};
use crate::batch::layout::{BASE_OFFSET, crc_of};
use crate::batch::{Batch, Compression, Refusal};

/// What the topic and the release the broker stands for add to the rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpRules {
    /// Kafka 2.0's rules on the set's shape (one entry if compressed or v2, one format throughout). Kafka 1.1 has none.
    pub strict: bool,
    /// Whether the release knows zstd (Kafka 2.1, KIP-110): one that does not cannot read a wrapper that names it.
    pub knows_zstd: bool,
    /// The most bytes one entry may have (`max.message.bytes`).
    pub max_message_bytes: u64,
    /// The most a wrapper's inner messages may inflate to.
    pub max_inflated: u64,
}

/// A message set, as the log will take it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Up {
    /// No whole entry: nothing is appended, and the partition is answered with no offset (-1), as Kafka answers it.
    Empty,
    /// One v2 batch, to be taken as a Produce v3 batch is.
    Batch(Bytes),
    /// The records, made one uncompressed v2 batch, and the codec the producer used (the topic's `compression.type`
    /// `producer` keeps it).
    Converted {
        /// The batch: base offset 0, no producer, CreateTime, the records' own timestamps (-1 for format v0).
        batch: Batch,
        /// The codec of the set's compressed entries (the last, as Kafka takes it), or none.
        source: Compression,
    },
}

/// Why a message set was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{reason}")]
pub struct UpError {
    /// How the partition is answered.
    pub refusal: Refusal,
    /// What was wrong, for the log.
    pub reason: String,
}

fn refused(refusal: Refusal, reason: impl Into<String>) -> UpError {
    UpError {
        refusal,
        reason: reason.into(),
    }
}

// fuzz:parser fuzz_batch
/// The message set of one partition of a Produce v0 to v2 request, held to Kafka's rules and made one batch.
///
/// # Errors
/// The first rule broken, in Kafka's order.
pub fn up(set: &Bytes, rules: &UpRules) -> Result<Up, UpError> {
    let (entries, source) = look(set, rules)?;
    let Some(first) = entries.first() else {
        return Ok(Up::Empty);
    };
    if rules.strict {
        if (first.magic() == 2 || source != Compression::None) && entries.len() > 1 {
            return Err(refused(
                Refusal::InvalidRecord,
                "Compressed outer record has more than one batch",
            ));
        }
        if let Some(other) = entries.iter().find(|e| e.magic() != first.magic()) {
            return Err(refused(
                Refusal::InvalidRecord,
                format!(
                    "Batch magic {} is not the same as the first batch's magic byte {}",
                    other.magic(),
                    first.magic()
                ),
            ));
        }
    }
    if let ([Entry::Batch(batch)], _) = (entries.as_slice(), source) {
        return Ok(Up::Batch(batch.clone()));
    }
    let mut recs = Vec::new();
    for entry in &entries {
        match entry {
            Entry::Message(m) => take_message(m, rules, &mut recs)?,
            // Kafka 1.1 converts a v2 batch among older entries with the rest (2.0 refuses the mix above).
            Entry::Batch(b) => take_batch(b, rules, &mut recs)?,
        }
    }
    if recs.is_empty() {
        return Ok(Up::Empty);
    }
    let batch = Batch::parse(v2_batch(&recs)).map_err(|e| {
        refused(
            Refusal::UnknownServerError,
            format!("the converted batch is not one: {e}"),
        )
    })?;
    Ok(Up::Converted { batch, source })
}

/// Every whole entry, each held to its size and checksum, and the codec of the compressed ones.
fn look(set: &Bytes, rules: &UpRules) -> Result<(Vec<Entry>, Compression), UpError> {
    let mut entries = Vec::new();
    let mut source = Compression::None;
    for entry in message::entries(set) {
        let entry = entry.map_err(|e| match e {
            Unreadable::Size(n) => refused(
                Refusal::CorruptMessage,
                format!("Record size {n} is less than the minimum record overhead (14)"),
            ),
            Unreadable::Magic(m) => refused(
                Refusal::CorruptMessage,
                format!("Invalid magic found in record: {m}"),
            ),
        })?;
        if let Entry::Batch(b) = &entry
            && b.get(BASE_OFFSET..BASE_OFFSET + 8) != Some(&[0; 8][..])
        {
            return Err(refused(
                Refusal::InvalidRecord,
                "a v2 batch from a client has a base offset other than 0",
            ));
        }
        if entry.bytes().len() as u64 > rules.max_message_bytes {
            return Err(refused(
                Refusal::MessageTooLarge,
                format!("a message of {} bytes", entry.bytes().len()),
            ));
        }
        let (valid, codec) = match &entry {
            Entry::Message(m) => (m.is_valid(), codec_of(m, rules)?),
            Entry::Batch(b) => (
                b.get(17..21).and_then(|c| <[u8; 4]>::try_from(c).ok()) == Some(crc_of(b).to_be_bytes()),
                crate::batch::Attributes::from_bits(
                    b.get(21..23).and_then(|a| <[u8; 2]>::try_from(a).ok()).map_or(0, u16::from_be_bytes),
                )
                .compression(),
            ),
        };
        if !valid {
            return Err(refused(Refusal::CorruptMessage, "Record is corrupt (stored crc does not match)"));
        }
        if codec != Compression::None {
            source = codec;
        }
        entries.push(entry);
    }
    Ok((entries, source))
}

/// The codec a message's attributes name: one Kafka does not know cannot be told apart (UNKNOWN_SERVER_ERROR, as
/// Kafka's `CompressionType.forId` throws), and a release before zstd does not know zstd.
fn codec_of(m: &Message, rules: &UpRules) -> Result<Compression, UpError> {
    match codec::of(m.codec()) {
        Compression::Unknown(n) => Err(refused(
            Refusal::UnknownServerError,
            format!("Unknown compression type id: {n}"),
        )),
        Compression::Zstd if !rules.knows_zstd => Err(refused(
            Refusal::UnknownServerError,
            "Unknown compression type id: 4",
        )),
        c => Ok(c),
    }
}

/// A message's records: itself, or a wrapper's inner messages.
fn take_message(m: &Message, rules: &UpRules, out: &mut Vec<Rec>) -> Result<(), UpError> {
    let codec = codec::of(m.codec());
    if codec == Compression::None {
        out.push(record_of(m, None)?);
        return Ok(());
    }
    if codec == Compression::Zstd {
        return Err(refused(
            Refusal::InvalidRecord,
            "Invalid wrapper compressionType found in legacy batch: ZSTD",
        ));
    }
    let (_, value) = m
        .key_value()
        .map_err(|_| refused(Refusal::CorruptMessage, "a wrapper's fields do not fit it"))?;
    let inner = codec::inflate(m.magic(), codec, value.as_deref().unwrap_or_default(), rules.max_inflated)
        .map_err(|e| match e {
            crate::batch::inflate::InflateError::TooLarge => {
                refused(Refusal::MessageTooLarge, "the inner messages inflate past the limit")
            }
            other => refused(Refusal::CorruptMessage, format!("the inner messages cannot be read: {other:?}")),
        })?;
    let inner = Bytes::from(inner);
    // The wrapper's time stands for every inner message's when it is the broker's (LogAppendTime).
    let stamped = m.log_append_time().then(|| m.timestamp());
    for entry in message::entries(&inner) {
        let Ok(Entry::Message(i)) = entry else {
            return Err(refused(Refusal::CorruptMessage, "an inner message that is not one"));
        };
        if i.magic() != m.magic() {
            return Err(refused(Refusal::InvalidRecord, "Log record magic does not match outer magic"));
        }
        if i.codec() != 0 {
            return Err(refused(
                Refusal::InvalidRecord,
                "Compressed outer record should not have an inner record with a compression attribute set",
            ));
        }
        if !i.is_valid() {
            return Err(refused(Refusal::CorruptMessage, "an inner message's checksum does not match"));
        }
        out.push(record_of(&i, stamped)?);
    }
    Ok(())
}

fn record_of(m: &Message, stamped: Option<i64>) -> Result<Rec, UpError> {
    let (key, value) = m
        .key_value()
        .map_err(|_| refused(Refusal::CorruptMessage, "a message's fields do not fit it"))?;
    Ok(Rec {
        offset: m.offset(),
        timestamp: stamped.unwrap_or_else(|| m.timestamp()),
        key,
        value,
        headers: Vec::new(),
    })
}

/// A v2 batch's records, among older entries (Kafka 1.1 only).
fn take_batch(b: &Bytes, rules: &UpRules, out: &mut Vec<Rec>) -> Result<(), UpError> {
    let batch = Batch::parse(b.clone()).map_err(|e| refused(e.refusal(), e.to_string()))?;
    let recs = records::read(&batch, rules.max_inflated).map_err(|e| match e {
        Unread::TooLarge => refused(Refusal::MessageTooLarge, "the records inflate past the limit"),
        Unread::Unsupported(c) => refused(Refusal::UnsupportedCompression, format!("{c:?}")),
        Unread::Corrupt(why) => refused(Refusal::CorruptMessage, why),
    })?;
    out.extend(recs);
    Ok(())
}

