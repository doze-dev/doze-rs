//! Which partition a keyed record goes to when the person leaves it to the key: Kafka's default partitioner
//! (`Utils.murmur2` of the key bytes, made positive, modulo the partition count), so that the console puts a key where a
//! Java client would. A record with no key has nothing to hash and goes to partition 0.

/// Kafka's `Utils.murmur2`.
#[must_use]
pub fn murmur2(data: &[u8]) -> i32 {
    const M: u32 = 0x5bd1_e995;
    const SEED: u32 = 0x9747_b28c;
    let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
    let mut h = SEED ^ len;
    let mut chunks = data.chunks_exact(4);
    for c in chunks.by_ref() {
        let Ok(word) = <[u8; 4]>::try_from(c) else {
            continue;
        };
        let mut k = u32::from_le_bytes(word);
        k = k.wrapping_mul(M);
        k ^= k >> 24;
        k = k.wrapping_mul(M);
        h = h.wrapping_mul(M) ^ k;
    }
    let rest = chunks.remainder();
    if !rest.is_empty() {
        for (i, b) in rest.iter().enumerate().rev() {
            h ^= u32::from(*b) << (8 * i);
        }
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    h as i32
}

/// The partition for `key` among `count`, as the default partitioner chooses; 0 for no key (or no partitions).
#[must_use]
pub fn for_key(key: Option<&[u8]>, count: i32) -> i32 {
    match key {
        Some(k) if count > 0 => {
            let positive = u32::from_ne_bytes(murmur2(k).to_ne_bytes()) & 0x7fff_ffff;
            i32::try_from(positive % count.unsigned_abs()).unwrap_or(0)
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hash_is_kafkas_on_its_own_test_vectors() {
        // From Kafka's UtilsTest.testMurmur2.
        for (input, want) in [
            ("21", -973_932_308),
            ("foobar", -790_332_482),
            ("a-little-bit-long-string", -985_981_536),
            ("a-little-bit-longer-string", -1_486_304_829),
            (
                "lkjh234lh9fiuh90y23oiuhsafujhadof229phr9h19h89h8",
                -58_897_971,
            ),
        ] {
            assert_eq!(murmur2(input.as_bytes()), want, "{input}");
        }
        assert_eq!(murmur2(&[b'a', b'b', b'c']), 479_470_107);
    }

    #[test]
    fn a_key_goes_where_the_default_partitioner_puts_it() {
        // murmur2("21") is -973932308: made positive it is 1173551340.
        assert_eq!(for_key(Some(b"21"), 3), 0);
        assert_eq!(for_key(Some(b"21"), 7), 1_173_551_340 % 7);
        assert_eq!(for_key(None, 3), 0);
        assert_eq!(for_key(Some(b"k"), 0), 0);
        assert_eq!(for_key(Some(b"k"), 1), 0);
        // Always inside the topic, for any count and any key.
        for count in [1, 2, 3, 7, 1000, i32::MAX] {
            for key in [
                "",
                "a",
                "ab",
                "abc",
                "abcd",
                "abcde",
                "ord-88420",
                "\u{e9}\u{e9}",
            ] {
                let p = for_key(Some(key.as_bytes()), count);
                assert!((0..count).contains(&p), "{key:?} {count} {p}");
            }
        }
    }
}
