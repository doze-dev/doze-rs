//! What a Fetch v0 to v3 read of v2 batches becomes: messages of format v0 (Fetch v0 and v1) or v1 (v2 and v3), as
//! Kafka down-converts them for an old consumer.
//!
//! Each batch is converted on its own (`RecordsUtil.downConvert`): a control batch (a transaction marker) has no form
//! there and is dropped; zstd cannot be written in either format (UNSUPPORTED_COMPRESSION_TYPE); an uncompressed batch
//! becomes one message per record, less those before the fetch offset; a compressed one becomes one wrapper in its codec
//! holding all its records (format v1 numbers the inner messages from 0, v0 by their offsets). Headers are lost, and the
//! records of an aborted transaction are not told apart (an old consumer reads uncommitted). Under LogAppendTime the
//! batch's time is every message's, and the type is marked (v1).
//!
//! How the converted bytes fill the answer is the release's:
//!
//! - Kafka 1.1 converts the read all at once and answers with what came out; a read that converts to nothing is answered
//!   as it was read (`FileRecords.downConvert`: an old client sees a batch it cannot read whole, and asks for more).
//! - From Kafka 2.0 (KIP-283, `LazyDownConversionRecords`) the answer is as long as the read, or as the first batch that
//!   converts to something if that is longer, and is written as it is converted: what is converted past that length is
//!   cut off (a consumer stops at a message cut short), and what falls short of it is filled with an "overflow" message
//!   (offset -1 and a length past the end, which a consumer drops). The first batch that converts is converted twice when
//!   it is not the first batch read: Kafka's chunked iterator steps past one batch for it, not past every batch it took to
//!   find it. A read with no whole batch is answered as read by Kafka 2 (`hasCompatibleMagic` holds of nothing), with
//!   nothing by Kafka 3 (which always converts), and a read that converts to nothing with one 12-byte overflow message.

use bytes::Bytes;

use super::codec;
use super::message::{self, LOG_OVERHEAD, Out};
use super::records::{self, Unread};
use super::{Magic, Rec};
use crate::batch::layout::declared_total;
use crate::batch::{Batch, Compression, TimestampType};

/// How the release the broker stands for fills the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Downconversion {
    /// Kafka 1.1: all at once.
    Eager,
    /// Kafka 2 (KIP-283): chunked; a read with no whole batch is answered as read.
    Chunked,
    /// Kafka 3: chunked; a read with no whole batch is answered with nothing.
    ChunkedAlways,
}

/// One partition's conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownRules {
    /// The format the Fetch version reads.
    pub to: Magic,
    /// The offset the consumer asked for: an uncompressed batch's records before it are left out.
    pub fetch_offset: i64,
    /// How the answer is filled.
    pub how: Downconversion,
    /// The most a compressed batch may inflate to.
    pub max_inflated: u64,
}

/// Why a read could not be converted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DownError {
    /// A zstd batch, which neither old format can hold (`UNSUPPORTED_COMPRESSION_TYPE`).
    #[error("Down-conversion of zstandard-compressed batches is not supported")]
    Zstd,
    /// A stored batch whose records cannot be read.
    #[error("a stored batch cannot be read: {0}")]
    Unreadable(String),
}

/// The bytes of a message that tells a consumer the rest of the answer is not for it.
const MIN_OVERFLOW: usize = LOG_OVERHEAD;
/// How much of the answer Kafka writes at a time (`LazyDownConversionRecordsSend.MAX_READ_SIZE`).
const MAX_READ_SIZE: usize = 16 << 10;
/// The length an overflow message claims at least (a v2 batch's header, `RECORD_BATCH_OVERHEAD`).
const BATCH_OVERHEAD: usize = 61;

// fuzz:parser fuzz_batch
/// `read`, the v2 batches a Fetch read for one partition (the last possibly cut short), as an old consumer is answered.
///
/// # Errors
/// A zstd batch where Kafka refuses the partition, or a stored batch that cannot be read.
pub fn down(read: &Bytes, rules: &DownRules) -> Result<Bytes, DownError> {
    let batches = whole_batches(read);
    if batches.is_empty() {
        return Ok(match rules.how {
            Downconversion::ChunkedAlways => Bytes::new(),
            Downconversion::Eager | Downconversion::Chunked => read.clone(),
        });
    }
    if rules.how == Downconversion::Eager {
        let mut out = Vec::new();
        for b in &batches {
            out.extend(convert(b, rules)?);
        }
        return Ok(if out.is_empty() {
            read.clone()
        } else {
            Bytes::from(out)
        });
    }
    // The first batch that converts to something sizes the answer, before anything is written.
    let mut first = None;
    for b in &batches {
        let c = convert(b, rules)?;
        if !c.is_empty() {
            first = Some(c);
            break;
        }
    }
    let Some(first) = first else {
        let mut out = Vec::with_capacity(MIN_OVERFLOW);
        overflow(&mut out, MIN_OVERFLOW);
        return Ok(Bytes::from(out));
    };
    let total = read.len().max(first.len());
    let mut out = first;
    // Then the rest, from the second batch read on. A zstd batch now (Kafka has written part of the answer and gives up
    // on the connection) ends the conversion: what is left is overflow.
    for b in batches.iter().skip(1) {
        if out.len() >= total {
            break;
        }
        match convert(b, rules) {
            Ok(c) => out.extend(c),
            Err(DownError::Zstd) => break,
            Err(e) => return Err(e),
        }
    }
    fit(&mut out, total);
    Ok(Bytes::from(out))
}

/// The whole batches at the front of `read`.
fn whole_batches(read: &Bytes) -> Vec<Batch> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(rest) = read.get(at..) {
        let Some(total) = declared_total(rest).filter(|t| *t <= rest.len()) else {
            break;
        };
        let Ok(batch) = Batch::parse_stored(read.slice(at..at + total)) else {
            break;
        };
        out.push(batch);
        at += total;
    }
    out
}

/// One batch, converted: nothing for a control batch or one with no records left.
fn convert(batch: &Batch, rules: &DownRules) -> Result<Vec<u8>, DownError> {
    if batch.attributes().is_control() {
        return Ok(Vec::new());
    }
    let codec = batch.attributes().compression();
    if codec == Compression::Zstd {
        return Err(DownError::Zstd);
    }
    let mut recs = records::read(batch, rules.max_inflated).map_err(|e| match e {
        Unread::TooLarge => DownError::Unreadable("it inflates past the limit".into()),
        Unread::Unsupported(c) => DownError::Unreadable(format!("no decoder for {c:?}")),
        Unread::Corrupt(why) => DownError::Unreadable(why),
    })?;
    if codec == Compression::None {
        recs.retain(|r| r.offset >= rules.fetch_offset);
    }
    let (Some(head), Some(last)) = (recs.first(), recs.last()) else {
        return Ok(Vec::new());
    };
    let lat = (batch.attributes().timestamp_type() == TimestampType::LogAppend)
        .then(|| batch.max_timestamp().get());
    let mut out = Vec::new();
    if codec == Compression::None {
        for r in &recs {
            message::write(&mut out, &out_of(rules.to, r, r.offset, lat));
        }
        return Ok(out);
    }
    let mut inner = Vec::new();
    for r in &recs {
        let offset = match rules.to {
            Magic::V0 => r.offset,
            Magic::V1 => r.offset - head.offset,
        };
        message::write(&mut inner, &out_of(rules.to, r, offset, lat));
    }
    let value = codec::deflate(rules.to, codec, &inner)
        .ok_or_else(|| DownError::Unreadable(format!("no encoder for {codec:?}")))?;
    let newest = recs.iter().map(|r| r.timestamp).max().unwrap_or(-1).max(-1);
    message::write(
        &mut out,
        &Out {
            magic: rules.to,
            offset: last.offset,
            timestamp: lat.unwrap_or(newest),
            log_append_time: lat.is_some(),
            codec: codec::number(codec),
            key: None,
            value: Some(&value),
        },
    );
    Ok(out)
}

fn out_of(magic: Magic, r: &Rec, offset: i64, lat: Option<i64>) -> Out<'_> {
    Out {
        magic,
        offset,
        timestamp: lat.unwrap_or(r.timestamp),
        log_append_time: lat.is_some(),
        codec: 0,
        key: r.key.as_deref(),
        value: r.value.as_deref(),
    }
}

/// `out` cut or filled to `total` bytes.
fn fit(out: &mut Vec<u8>, total: usize) {
    if out.len() >= total {
        out.truncate(total);
    } else {
        let short = total - out.len();
        overflow(out, short);
    }
}

/// `remaining` bytes of overflow messages, as Kafka writes them (`LazyDownConversionRecordsSend.buildOverflowBatch`): each
/// an offset of -1 and a length one past what is left (at least a batch header's), then zeros, up to 16 KiB at a time.
fn overflow(out: &mut Vec<u8>, mut remaining: usize) {
    while remaining > 0 {
        let chunk = (remaining + 1).clamp(MIN_OVERFLOW, MAX_READ_SIZE);
        let claimed = i32::try_from((remaining + 1).max(BATCH_OVERHEAD)).unwrap_or(i32::MAX);
        let mut message = vec![0_u8; chunk];
        if let Some(offset) = message.get_mut(..8) {
            offset.copy_from_slice(&(-1_i64).to_be_bytes());
        }
        if let Some(length) = message.get_mut(8..12) {
            length.copy_from_slice(&claimed.to_be_bytes());
        }
        let take = chunk.min(remaining);
        out.extend_from_slice(message.get(..take).unwrap_or_default());
        remaining -= take;
    }
}
