//! Why a batch is refused, and what a Produce request is told about it.

use super::layout::{HEADER_LEN, MAGIC_V2};

/// How a refused batch is answered, as Kafka answers it. The broker maps each to its error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// `CORRUPT_MESSAGE`: the bytes are not a batch, or fail their checksum.
    CorruptMessage,
    /// `INVALID_RECORD`: a batch whose contents break a rule.
    InvalidRecord,
    /// `UNKNOWN_SERVER_ERROR`: a compression codec this broker does not know.
    UnknownServerError,
    /// `MESSAGE_TOO_LARGE`: over `max.message.bytes`.
    MessageTooLarge,
    /// `UNSUPPORTED_COMPRESSION_TYPE`: zstd in a request older than v7.
    UnsupportedCompression,
}

/// Why a batch was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BatchError {
    /// Fewer bytes than a v2 header.
    #[error("{0} bytes is shorter than a record batch header ({HEADER_LEN})")]
    TooShort(usize),
    /// A batch format this broker does not store.
    #[error("record batch magic {0}, want {MAGIC_V2}")]
    Magic(i8),
    /// The declared length is not the bytes present.
    #[error("the batch declares {declared} bytes and {present} are present")]
    Length {
        /// What the header says.
        declared: i64,
        /// What there is.
        present: usize,
    },
    /// The stored checksum is not the batch's.
    #[error("the batch's CRC is {stored:#010x} and its contents give {computed:#010x}")]
    Crc {
        /// What the header says.
        stored: u32,
        /// What the contents give.
        computed: u32,
    },
    /// A compression codec number that is not one of Kafka's five.
    #[error("unknown compression codec {0}")]
    UnknownCodec(u8),
    /// The record count is not positive, or disagrees with `lastOffsetDelta`.
    #[error("record count {count} disagrees with lastOffsetDelta {last_offset_delta}")]
    Count {
        /// The count in the header.
        count: i32,
        /// The last offset delta in the header.
        last_offset_delta: i32,
    },
    /// More records than the bytes that carry them could hold: the log-end offset advances by the count,
    /// so a lying one would skip offsets.
    #[error("{count} records cannot fit in {body} bytes")]
    CountExceedsBody {
        /// The count in the header.
        count: i32,
        /// The bytes after the header.
        body: usize,
    },
    /// More records than one compressed batch may claim.
    #[error("{0} records in one batch")]
    TooManyRecords(i32),
    /// A run of batches that ends inside one.
    #[error("a batch is cut short at byte {0}")]
    Truncated(usize),
    /// A batch in a run of them that is not valid.
    #[error("batch at byte {at}: {error}")]
    InPayload {
        /// Where the batch starts.
        at: usize,
        /// What is wrong with it.
        error: Box<BatchError>,
    },
}

impl BatchError {
    /// How Kafka answers this refusal. The order the checks run in (shape, checksum, codec, count)
    /// decides which of several faults a request is told about.
    #[must_use]
    pub fn refusal(&self) -> Refusal {
        match self {
            Self::TooShort(_)
            | Self::Magic(_)
            | Self::Length { .. }
            | Self::Crc { .. }
            | Self::Truncated(_) => Refusal::CorruptMessage,
            Self::UnknownCodec(_) => Refusal::UnknownServerError,
            Self::Count { .. } | Self::CountExceedsBody { .. } | Self::TooManyRecords(_) => {
                Refusal::InvalidRecord
            }
            Self::InPayload { error, .. } => error.refusal(),
        }
    }
}
