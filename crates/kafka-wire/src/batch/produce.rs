//! The shape of a Produce request's records, as Kafka takes them: for each partition, one v2 batch.
//!
//! Kafka reads the first batch, refuses a second whole batch, and drops bytes after a batch that were
//! never a whole one. The checks run in an order that decides which fault a client hears about, and it is
//! the order Kafka's recordings show.

use bytes::Bytes;

use super::layout::{self, LENGTH_OVERHEAD, MAGIC_V2};
use super::{Batch, BatchError, Compression, Refusal};
use crate::ids::Offset;

/// What a topic and the request's version add to the rules every batch is held to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProduceRules {
    /// The version of the Produce request: zstd came with v7.
    pub version: i16,
    /// The most bytes a batch may have (`max.message.bytes`).
    pub max_batch_bytes: u64,
}

/// The smallest size a batch may declare: Kafka's reader refuses less whatever the format.
const MIN_DECLARED: i32 = 14;
/// Bytes through the magic field: enough to tell what a batch claims to be.
const THROUGH_MAGIC: usize = 17;

/// Why a partition's records were refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProduceError {
    /// The records hold no whole batch.
    #[error("the records hold no whole record batch ({0} bytes)")]
    NoWholeBatch(usize),
    /// A batch that declares fewer bytes than any can be.
    #[error("a record batch that says it is {0} bytes long, less than any can be")]
    DeclaredSize(i32),
    /// A magic byte that is not a Kafka batch format at all.
    #[error("a record batch of format version {0}")]
    UnknownFormat(i8),
    /// An older batch format (0 or 1); only v2 is taken.
    #[error("a record batch of format version {0}; only version 2 is taken")]
    OldFormat(i8),
    /// More than one batch: a Produce request carries exactly one per partition.
    #[error("more than one record batch for a partition; a Produce request carries exactly one")]
    MoreThanOne,
    /// zstd in a request older than v7.
    #[error("zstd needs Produce version 7 or later, and this is version {0}")]
    Zstd(i16),
    /// A batch over `max.message.bytes`.
    #[error("a record batch of {size} bytes is larger than max.message.bytes ({limit})")]
    TooLarge {
        /// The batch's size.
        size: usize,
        /// The limit.
        limit: u64,
    },
    /// A producer writes base offset 0; the broker assigns the real one.
    #[error("the batch's base offset is {0}; a producer writes 0")]
    BaseOffset(i64),
    /// The batch itself is not valid.
    #[error(transparent)]
    Batch(#[from] BatchError),
    /// Only a broker writes control batches.
    #[error("clients may not write control batches")]
    ControlBatch,
    /// A producer id with no sequence number.
    #[error("a batch with a producer id ({0}) and no sequence number")]
    NoSequence(i64),
    /// A fault found while looking for a second batch after the first, which Kafka answers without its text.
    #[error("after the first batch: {0}")]
    AfterTheBatch(Box<ProduceError>),
}

impl ProduceError {
    /// How Kafka answers this refusal.
    #[must_use]
    pub fn refusal(&self) -> Refusal {
        match self {
            Self::DeclaredSize(_) | Self::UnknownFormat(_) => Refusal::CorruptMessage,
            Self::Zstd(_) => Refusal::UnsupportedCompression,
            Self::TooLarge { .. } => Refusal::MessageTooLarge,
            Self::Batch(e) => e.refusal(),
            Self::AfterTheBatch(inner) => inner.refusal(),
            _ => Refusal::InvalidRecord,
        }
    }
}

impl ProduceError {
    /// Whether Kafka refuses this before it has asked the partition anything, so that its answer carries no
    /// log start offset (-1). A refusal about the batch's header or contents is made with the log in hand.
    #[must_use]
    pub fn before_log(&self) -> bool {
        match self {
            Self::NoWholeBatch(_)
            | Self::DeclaredSize(_)
            | Self::UnknownFormat(_)
            | Self::OldFormat(_)
            | Self::MoreThanOne
            | Self::Zstd(_)
            | Self::TooLarge { .. } => true,
            Self::AfterTheBatch(inner) => inner.before_log(),
            Self::Batch(e) => matches!(
                e,
                BatchError::TooShort(_)
                    | BatchError::Length { .. }
                    | BatchError::Magic(_)
                    | BatchError::Crc { .. }
                    | BatchError::Truncated(_)
            ),
            _ => false,
        }
    }

    /// The text Kafka's answer carries for this refusal, where it has one.
    #[must_use]
    pub fn kafka_message(&self) -> Option<String> {
        match self {
            Self::DeclaredSize(n) => Some(format!(
                "Record size {n} is less than the minimum record overhead ({MIN_DECLARED})"
            )),
            Self::UnknownFormat(m) => Some(format!("Invalid magic found in record: {m}")),
            _ => None,
        }
    }
}

/// The first batch of `records` and what follows it, or `None` if there is no whole batch yet.
fn first(records: &Bytes) -> Result<Option<(Bytes, Bytes)>, ProduceError> {
    if records.len() < THROUGH_MAGIC {
        return Ok(None);
    }
    let size = records
        .get(8..12)
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map_or(0, i32::from_be_bytes);
    if size < MIN_DECLARED {
        return Err(ProduceError::DeclaredSize(size));
    }
    let total = usize::try_from(i64::from(size) + LENGTH_OVERHEAD).unwrap_or(usize::MAX);
    if records.len() < total {
        return Ok(None); // the batch says it is longer than what was sent
    }
    let magic = records.get(16).copied().map_or(0, |b| b as i8);
    if !(0..=MAGIC_V2).contains(&magic) {
        return Err(ProduceError::UnknownFormat(magic));
    }
    Ok(Some((records.slice(..total), records.slice(total..))))
}

impl Batch {
    /// The one batch of a partition's Produce records, held to Kafka's rules.
    ///
    /// # Errors
    /// The first rule broken, in the order Kafka checks them.
    pub fn from_produce(records: &Bytes, rules: ProduceRules) -> Result<Self, ProduceError> {
        let (batch, rest) = first(records)?.ok_or(ProduceError::NoWholeBatch(records.len()))?;
        let magic = batch.get(16).copied().map_or(0, |b| b as i8);
        if magic != MAGIC_V2 {
            return Err(ProduceError::OldFormat(magic));
        }
        if rules.version < 7 && declared_compression(&batch) == Some(Compression::Zstd) {
            return Err(ProduceError::Zstd(rules.version));
        }
        if first(&rest)
            .map_err(|e| ProduceError::AfterTheBatch(Box::new(e)))?
            .is_some()
        {
            return Err(ProduceError::MoreThanOne);
        }
        let base = batch
            .get(..8)
            .and_then(|b| <[u8; 8]>::try_from(b).ok())
            .map_or(0, i64::from_be_bytes);
        if Offset::new(base) != Some(Offset::ZERO) {
            return Err(ProduceError::BaseOffset(base));
        }
        if u64::try_from(batch.len()).is_ok_and(|n| n > rules.max_batch_bytes) {
            return Err(ProduceError::TooLarge {
                size: batch.len(),
                limit: rules.max_batch_bytes,
            });
        }
        let batch = Self::parse(batch)?;
        if batch.attributes().is_control() {
            return Err(ProduceError::ControlBatch);
        }
        match batch.producer() {
            Some(p) if p.base_sequence.is_none() => Err(ProduceError::NoSequence(p.id.get())),
            _ => Ok(batch),
        }
    }

    /// Whether a partition's first batch is transactional, which the request layer asks of every
    /// partition before it handles any: Kafka fails the whole request on a first batch it cannot tell apart.
    ///
    /// # Errors
    /// A first batch with an impossible size or format byte, which refuses every partition.
    pub fn first_is_transactional(records: &Bytes) -> Result<bool, ProduceError> {
        let Some((batch, _)) = first(records)? else {
            return Ok(false);
        };
        let attributes = layout::ATTRIBUTES..layout::ATTRIBUTES + 2;
        Ok(batch.get(16).copied().map(|b| b as i8) == Some(MAGIC_V2)
            && batch
                .get(attributes)
                .and_then(|b| <[u8; 2]>::try_from(b).ok())
                .is_some_and(|b| {
                    super::Attributes::from_bits(u16::from_be_bytes(b)).is_transactional()
                }))
    }
}

/// The codec a batch's attributes name, if the batch is long enough to have them.
fn declared_compression(batch: &Bytes) -> Option<Compression> {
    if batch.len() < super::HEADER_LEN {
        return None;
    }
    let bits = batch
        .get(layout::ATTRIBUTES..layout::ATTRIBUTES + 2)
        .and_then(|b| <[u8; 2]>::try_from(b).ok())?;
    Some(super::Attributes::from_bits(u16::from_be_bytes(bits)).compression())
}

#[cfg(test)]
mod tests;
