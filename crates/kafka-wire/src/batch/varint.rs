//! The zigzag varint of the v2 record format (Kafka's `varint` and `varlong`, `ByteUtils.readVarlong`): seven bits a
//! byte, low bits first, the high bit saying another byte follows, and the sign in the lowest bit of what results.
//!
//! One reader and one writer for every record field of the crate. The reader holds to the strict rule: a number is at
//! most ten bytes, and the tenth may carry only the one bit a 64-bit number has left, so no two byte strings read as
//! the same number by dropping bits off the top.

use bytes::BufMut;

/// Why a varint could not be read from a source of bytes.
#[derive(Debug)]
pub(super) enum Bad<E> {
    /// More than ten bytes, or a tenth that carries more than one bit: no 64-bit number.
    TooLong,
    /// The source could not give the next byte.
    Source(E),
}

// fuzz:parser fuzz_batch
/// A varint pulled a byte at a time from `next`.
///
/// # Errors
/// [`Bad::Source`] with the source's error, or [`Bad::TooLong`].
pub(super) fn pull<E>(mut next: impl FnMut() -> Result<u8, E>) -> Result<i64, Bad<E>> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = next().map_err(Bad::Source)?;
        // The tenth byte may carry only the one bit that is left.
        if shift == 63 && byte > 1 {
            return Err(Bad::TooLong);
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(unzigzag(value));
        }
    }
    Err(Bad::TooLong)
}

/// A varint off the front of `bytes`, which is moved past it; `None` (and `bytes` as it was) if there is none.
pub(super) fn read(bytes: &mut &[u8]) -> Option<i64> {
    let mut rest = *bytes;
    let value = pull(|| {
        let (&byte, after) = rest.split_first().ok_or(())?;
        rest = after;
        Ok::<u8, ()>(byte)
    })
    .ok()?;
    *bytes = rest;
    Some(value)
}

/// `v` written to the end of `out`.
pub(super) fn write(out: &mut impl BufMut, v: i64) {
    let mut u = ((v << 1) ^ (v >> 63)).cast_unsigned();
    // Each `as u8` keeps exactly the seven bits masked off (or, last, all of `u`, which is under 0x80 by then).
    while u >= 0x80 {
        out.put_u8((u & 0x7f) as u8 | 0x80);
        u >>= 7;
    }
    out.put_u8(u as u8);
}

/// The signed number a zigzag-encoded one stands for: the low bit is the sign.
fn unzigzag(u: u64) -> i64 {
    (u >> 1).cast_signed() ^ -(u & 1).cast_signed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn written(v: i64) -> Vec<u8> {
        let mut out = Vec::new();
        write(&mut out, v);
        out
    }

    #[test]
    fn small_numbers_are_kafkas_bytes() {
        // ByteUtils.writeVarlong: 0 -> 00, -1 -> 01, 1 -> 02, 63 -> 7e, -64 -> 7f, 64 -> 80 01, 300 -> d8 04.
        for (v, want) in [
            (0_i64, &[0x00][..]),
            (-1, &[0x01]),
            (1, &[0x02]),
            (63, &[0x7e]),
            (-64, &[0x7f]),
            (64, &[0x80, 0x01]),
            (300, &[0xd8, 0x04]),
        ] {
            assert_eq!(written(v), want, "{v}");
        }
    }

    #[test]
    fn the_extremes_take_ten_bytes_and_read_back() {
        for v in [i64::MAX, i64::MIN] {
            let bytes = written(v);
            assert_eq!(bytes.len(), 10);
            assert_eq!(bytes.last(), Some(&1));
            let mut rest = bytes.as_slice();
            assert_eq!(read(&mut rest), Some(v));
            assert!(rest.is_empty());
        }
    }

    #[test]
    fn a_tenth_byte_with_more_than_the_last_bit_is_no_number() {
        let mut bytes = vec![0xff_u8; 9];
        bytes.push(0x02);
        let mut rest = bytes.as_slice();
        assert_eq!(read(&mut rest), None);
        assert_eq!(
            rest.len(),
            10,
            "nothing is taken from a slice that holds no number"
        );
        // With only the last bit, it is i64::MIN.
        bytes[9] = 0x01;
        assert_eq!(read(&mut bytes.as_slice()), Some(i64::MIN));
    }

    #[test]
    fn eleven_bytes_is_no_number_and_neither_is_a_cut_one() {
        let mut long = vec![0x80_u8; 10];
        long.push(0x00);
        assert_eq!(read(&mut long.as_slice()), None);
        assert_eq!(read(&mut [0x80_u8, 0x80].as_slice()), None);
        assert_eq!(read(&mut [].as_slice()), None);
    }

    #[test]
    fn pull_says_why() {
        let mut bytes = [0x80_u8].into_iter();
        assert!(matches!(
            pull(|| bytes.next().ok_or("cut")),
            Err(Bad::Source("cut"))
        ));
        let mut bytes = std::iter::repeat(0x80_u8);
        assert!(matches!(
            pull(|| bytes.next().ok_or("cut")),
            Err(Bad::TooLong)
        ));
    }

    proptest! {
        #[test]
        fn every_number_round_trips_and_leaves_what_follows(v: i64, tail: Vec<u8>) {
            let mut bytes = written(v);
            let len = bytes.len();
            bytes.extend(&tail);
            let mut rest = bytes.as_slice();
            prop_assert_eq!(read(&mut rest), Some(v));
            prop_assert_eq!(rest, tail.as_slice());
            prop_assert!((1..=10).contains(&len));
        }

        #[test]
        fn any_bytes_read_without_a_panic_and_reread_the_same(bytes: Vec<u8>) {
            let mut rest = bytes.as_slice();
            if let Some(v) = read(&mut rest) {
                // What was read is the shortest form unless the sender padded it; either way it writes back to a
                // number that reads the same.
                prop_assert_eq!(read(&mut written(v).as_slice()), Some(v));
            }
        }
    }
}
