//! Where the fields a broker rewrites sit in a v2 batch, and the two things computed from the bytes:
//! the checksum and the batch's declared size. Reading the header is [`super::header`].

pub(super) const BASE_OFFSET: usize = 0;
pub(super) const LENGTH: usize = 8;
pub(super) const CRC: usize = 17;
pub(super) const ATTRIBUTES: usize = 21;
/// Through `lastOffsetDelta`: the fields every batch has, whatever its format.
const THROUGH_LAST_OFFSET_DELTA: usize = 27;
pub(super) const MAX_TIMESTAMP: usize = 35;

/// The whole v2 header, through the record count.
pub(super) const HEADER_LEN: usize = 61;
/// `baseOffset` and `batchLength` are not counted by `batchLength`.
pub(super) const LENGTH_OVERHEAD: i64 = 12;
/// The only record batch format this broker stores.
pub(super) const MAGIC_V2: i8 = 2;

/// The CRC32C of a batch from its attributes field to the end: what the stored checksum must be.
pub(super) fn crc_of(b: &[u8]) -> u32 {
    crc32c::crc32c(b.get(ATTRIBUTES..).unwrap_or_default())
}

/// How many bytes the batch at the start of `b` says it takes, if `b` holds enough header to say.
pub(super) fn declared_total(b: &[u8]) -> Option<usize> {
    if b.len() < THROUGH_LAST_OFFSET_DELTA {
        return None;
    }
    let length = i64::from(i32::from_be_bytes(
        b.get(LENGTH..LENGTH + 4)?.try_into().ok()?,
    ));
    // The length must at least cover the header fields it counts.
    if length < i64::try_from(THROUGH_LAST_OFFSET_DELTA).ok()? - LENGTH_OVERHEAD {
        return None;
    }
    usize::try_from(length + LENGTH_OVERHEAD).ok()
}
