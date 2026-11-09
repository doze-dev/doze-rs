//! The attributes field: codec, timestamp type and the transaction bits.

const COMPRESSION_MASK: u16 = 0x07;
const TIMESTAMP_TYPE: u16 = 0x08;
const TRANSACTIONAL: u16 = 0x10;
const CONTROL: u16 = 0x20;
const DELETE_HORIZON: u16 = 0x40;

/// How a batch's records are compressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Compression {
    /// Not compressed.
    None,
    /// gzip.
    Gzip,
    /// snappy.
    Snappy,
    /// LZ4.
    Lz4,
    /// zstd.
    Zstd,
    /// A codec number this broker does not know.
    Unknown(u8),
}

/// Whose clock the records' timestamps are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TimestampType {
    /// The producer's (CreateTime).
    Create,
    /// The broker's, set at append (LogAppendTime).
    LogAppend,
}

/// A batch's attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attributes(u16);

impl Attributes {
    /// The attributes as written to a batch.
    #[must_use]
    pub fn bits(self) -> u16 {
        self.0
    }

    /// Attributes from their bits.
    #[must_use]
    pub fn from_bits(bits: u16) -> Self {
        Self(bits)
    }

    /// How the records are compressed.
    #[must_use]
    pub fn compression(self) -> Compression {
        match (self.0 & COMPRESSION_MASK) as u8 {
            0 => Compression::None,
            1 => Compression::Gzip,
            2 => Compression::Snappy,
            3 => Compression::Lz4,
            4 => Compression::Zstd,
            n => Compression::Unknown(n),
        }
    }

    /// Whose clock the timestamps are.
    #[must_use]
    pub fn timestamp_type(self) -> TimestampType {
        if self.0 & TIMESTAMP_TYPE == 0 {
            TimestampType::Create
        } else {
            TimestampType::LogAppend
        }
    }

    /// Whether the batch belongs to a transaction.
    #[must_use]
    pub fn is_transactional(self) -> bool {
        self.0 & TRANSACTIONAL != 0
    }

    /// Whether the batch is a control batch (transaction markers), which only a broker writes.
    #[must_use]
    pub fn is_control(self) -> bool {
        self.0 & CONTROL != 0
    }

    /// Whether the batch's first timestamp is the time its tombstones and markers may be removed after (compaction sets it,
    /// and the records' timestamps are relative to it).
    #[must_use]
    pub fn has_delete_horizon(self) -> bool {
        self.0 & DELETE_HORIZON != 0
    }

    /// These attributes with the delete horizon flag set.
    pub(super) fn with_delete_horizon(self) -> Self {
        Self(self.0 | DELETE_HORIZON)
    }

    /// These attributes with no compression.
    pub(super) fn uncompressed(self) -> Self {
        Self(self.0 & !COMPRESSION_MASK)
    }

    /// These attributes with the timestamp type set to LogAppendTime.
    pub(super) fn with_log_append_time(self) -> Self {
        Self(self.0 | TIMESTAMP_TYPE)
    }
}
