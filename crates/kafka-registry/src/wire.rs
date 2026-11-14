//! The bytes a Confluent serializer puts in a Kafka record value:
//!
//! ```text
//! byte 0        magic, 0
//! bytes 1-4     the schema's id, big-endian
//! Protobuf only the message index: a zig-zag varint count, then that many zig-zag varints naming which message
//!               of the schema (a count of 0 stands for [0], the first message)
//! the rest      the payload: Avro binary, Protobuf binary, or JSON text
//! ```

/// The magic byte.
pub const MAGIC: u8 = 0;

/// The most message indexes a value may name: a path of nested messages deeper than this is not a schema anyone wrote.
pub const MAX_MESSAGE_INDEXES: usize = 128;

/// Why bytes are not in the wire format.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    #[error("not in the registry's wire format")]
    NotFramed,
    #[error("bad message index")]
    BadIndex,
}

/// The schema id a value names and the bytes after the header.
///
/// # Errors
/// Fewer than five bytes, or another magic byte.
// fuzz:parser wire
pub fn split(value: &[u8]) -> Result<(u32, &[u8]), WireError> {
    match value {
        [MAGIC, a, b, c, d, rest @ ..] => Ok((u32::from_be_bytes([*a, *b, *c, *d]), rest)),
        _ => Err(WireError::NotFramed),
    }
}

/// The header for schema `id`.
#[must_use]
pub fn header(id: u32) -> [u8; 5] {
    let [a, b, c, d] = id.to_be_bytes();
    [MAGIC, a, b, c, d]
}

/// One zig-zag varint (Protobuf's `sint64`, as Go's `binary.Varint` reads it) and the bytes after it.
fn varint(b: &[u8]) -> Option<(i64, &[u8])> {
    let mut v: u64 = 0;
    for (i, &c) in b.iter().enumerate().take(10) {
        if i == 9 && c > 1 {
            return None;
        }
        v |= u64::from(c & 0x7f) << (7 * i);
        if c & 0x80 == 0 {
            let n = i64::try_from(v >> 1).unwrap_or(i64::MAX) ^ -i64::try_from(v & 1).unwrap_or(0);
            return Some((n, b.get(i + 1..)?));
        }
    }
    None
}

fn put_varint(out: &mut Vec<u8>, n: i64) {
    let mut v = ((n << 1) ^ (n >> 63)).cast_unsigned();
    while v >= 0x80 {
        out.push(u8::try_from(v & 0x7f).unwrap_or(0) | 0x80);
        v >>= 7;
    }
    out.push(u8::try_from(v).unwrap_or(0));
}

/// A Protobuf value's message index and the payload after it.
///
/// # Errors
/// A count or index that is not a varint, negative, or past [`MAX_MESSAGE_INDEXES`].
// fuzz:parser wire
pub fn message_indexes(b: &[u8]) -> Result<(Vec<usize>, &[u8]), WireError> {
    let (count, mut rest) = varint(b).ok_or(WireError::BadIndex)?;
    if count == 0 {
        return Ok((vec![0], rest));
    }
    let count = usize::try_from(count).map_err(|_| WireError::BadIndex)?;
    if count > MAX_MESSAGE_INDEXES {
        return Err(WireError::BadIndex);
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let (i, after) = varint(rest).ok_or(WireError::BadIndex)?;
        out.push(usize::try_from(i).map_err(|_| WireError::BadIndex)?);
        rest = after;
    }
    Ok((out, rest))
}

/// Writes a message index as Confluent's serializers do: `[0]` as a single 0.
pub fn put_message_indexes(out: &mut Vec<u8>, indexes: &[usize]) {
    if indexes == [0] {
        out.push(0);
        return;
    }
    put_varint(out, i64::try_from(indexes.len()).unwrap_or(i64::MAX));
    for &i in indexes {
        put_varint(out, i64::try_from(i).unwrap_or(i64::MAX));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_names_the_id_and_anything_else_is_not_framed() {
        let mut v = header(0x0102_0304).to_vec();
        v.extend(b"payload");
        assert_eq!(split(&v), Ok((0x0102_0304, &b"payload"[..])));
        assert_eq!(split(&[0, 0, 0, 1]), Err(WireError::NotFramed));
        assert_eq!(split(&[1, 0, 0, 0, 1]), Err(WireError::NotFramed));
        assert_eq!(split(&header(7)), Ok((7, &b""[..])));
    }

    #[test]
    fn message_indexes_round_trip_and_zero_is_the_first_message() {
        for idx in [vec![0], vec![1], vec![0, 2], vec![3, 0, 70]] {
            let mut b = Vec::new();
            put_message_indexes(&mut b, &idx);
            b.push(0xAA);
            assert_eq!(message_indexes(&b), Ok((idx, &[0xAA][..])));
        }
        assert_eq!(message_indexes(&[0]), Ok((vec![0], &b""[..])));
        // A negative count, a negative index, a truncated varint, an absurd count.
        assert!(message_indexes(&[1]).is_err());
        assert!(message_indexes(&[2, 1]).is_err());
        assert!(message_indexes(&[0x80]).is_err());
        assert!(message_indexes(&[0xFE, 0x03]).is_err());
    }
}
