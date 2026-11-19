//! The two checksums the older formats need and the newer one does not: CRC-32 (IEEE, the polynomial of zip and of
//! `java.util.zip.CRC32`), which seals every message of format v0 and v1, and xxHash32, which seals an LZ4 frame's
//! descriptor. Both are small enough to write out; neither is worth a crate.

/// The CRC-32 table of the reflected polynomial 0xEDB88320, made at compile time (where `n` is below 256 by the loop, and an
/// index out of range would fail the build, not a request).
#[allow(clippy::indexing_slicing)]
const CRC_TABLE: [u32; 256] = {
    let mut table = [0_u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 == 1 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
};

/// The CRC-32 of `bytes`, as `java.util.zip.CRC32` gives it.
pub(super) fn crc32(bytes: &[u8]) -> u32 {
    let mut c = !0_u32;
    for &b in bytes {
        let index = ((c ^ u32::from(b)) & 0xff) as usize;
        c = CRC_TABLE.get(index).copied().unwrap_or(0) ^ (c >> 8);
    }
    !c
}

const P1: u32 = 0x9E37_79B1;
const P2: u32 = 0x85EB_CA77;
const P3: u32 = 0xC2B2_AE3D;
const P4: u32 = 0x27D4_EB2F;
const P5: u32 = 0x1656_67B1;

fn word(bytes: &[u8]) -> u32 {
    let mut w = [0_u8; 4];
    for (dst, src) in w.iter_mut().zip(bytes) {
        *dst = *src;
    }
    u32::from_le_bytes(w)
}

fn round(acc: u32, input: u32) -> u32 {
    acc.wrapping_add(input.wrapping_mul(P2))
        .rotate_left(13)
        .wrapping_mul(P1)
}

/// xxHash32 of `bytes` with seed 0 (what an LZ4 frame's header checksum is taken from).
pub(super) fn xxh32(bytes: &[u8]) -> u32 {
    let len = bytes.len();
    let mut rest = bytes;
    let mut h = if len >= 16 {
        let mut v = [P1.wrapping_add(P2), P2, 0, 0_u32.wrapping_sub(P1)];
        while rest.len() >= 16 {
            let (stripe, after) = rest.split_at(16);
            for (i, acc) in v.iter_mut().enumerate() {
                *acc = round(*acc, word(stripe.get(i * 4..i * 4 + 4).unwrap_or_default()));
            }
            rest = after;
        }
        let [a, b, c, d] = v;
        a.rotate_left(1)
            .wrapping_add(b.rotate_left(7))
            .wrapping_add(c.rotate_left(12))
            .wrapping_add(d.rotate_left(18))
    } else {
        P5
    };
    h = h.wrapping_add(u32::try_from(len).unwrap_or(u32::MAX));
    while rest.len() >= 4 {
        let (w, after) = rest.split_at(4);
        h = h
            .wrapping_add(word(w).wrapping_mul(P3))
            .rotate_left(17)
            .wrapping_mul(P4);
        rest = after;
    }
    for &b in rest {
        h = h
            .wrapping_add(u32::from(b).wrapping_mul(P5))
            .rotate_left(11)
            .wrapping_mul(P1);
    }
    h ^= h >> 15;
    h = h.wrapping_mul(P2);
    h ^= h >> 13;
    h = h.wrapping_mul(P3);
    h ^ (h >> 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_is_zips() {
        // The check value of CRC-32/ISO-HDLC, and of an empty input.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
        assert_eq!(
            crc32(b"The quick brown fox jumps over the lazy dog"),
            0x414F_A339
        );
    }

    #[test]
    fn xxh32_is_the_reference() {
        // xxHash's own test values (seed 0).
        assert_eq!(xxh32(b""), 0x02CC_5D05);
        assert_eq!(xxh32(b"a"), 0x550D_7456);
        assert_eq!(xxh32(b"abc"), 0x32D1_53FF);
        assert_eq!(
            xxh32(b"Nobody inspects the spammish repetition"),
            0xE229_3B2F
        );
    }
}
