//! Standard base64 (RFC 4648 §4), as the AWS JSON protocols carry binary values. Decoding takes the padding or leaves it.

const SET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Not a base64 character.
const BAD: u8 = 0xff;

/// The value of every byte as a base64 character: one lookup per character. A search of `SET` was up to 64 compares a character,
/// and most of the server's time in a 5 MB Kinesis `PutRecords` (docs/port/weight.md, "Workstream 5").
// WHY: a const block cannot call `get_mut`; `i` is below 64 and a `u8` below 256, so neither index can miss.
#[allow(clippy::indexing_slicing)]
const VALUE: [u8; 256] = {
    let mut t = [BAD; 256];
    let mut i = 0;
    while i < 64 {
        t[SET[i] as usize] = i as u8;
        i += 1;
    }
    t
};

/// The base64 character for the six bits of `n` at `shift`.
fn char_at(n: u32, shift: u32) -> u8 {
    SET.get((n >> shift & 63) as usize).copied().unwrap_or(b'A')
}

/// The value of base64 character `c`, or [`BAD`].
fn value_of(c: u8) -> u8 {
    VALUE.get(usize::from(c)).copied().unwrap_or(BAD)
}

/// `bytes` as padded base64.
#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    let mut out = Vec::with_capacity(bytes.len().div_ceil(3) * 4);
    let (chunks, rest) = bytes.as_chunks::<3>();
    for &[a, b, c] in chunks {
        let n = u32::from(a) << 16 | u32::from(b) << 8 | u32::from(c);
        out.extend_from_slice(&[char_at(n, 18), char_at(n, 12), char_at(n, 6), char_at(n, 0)]);
    }
    match *rest {
        [a] => {
            let n = u32::from(a) << 16;
            out.extend_from_slice(&[char_at(n, 18), char_at(n, 12), b'=', b'=']);
        }
        [a, b] => {
            let n = u32::from(a) << 16 | u32::from(b) << 8;
            out.extend_from_slice(&[char_at(n, 18), char_at(n, 12), char_at(n, 6), b'=']);
        }
        _ => {}
    }
    // Every byte pushed is one of `SET` or `=`.
    String::from_utf8(out).unwrap_or_default()
}

// fuzz:parser fuzz_base64
/// The bytes `text` stands for, or `None` if it is not base64. Decoding stops at the first `=`.
#[must_use]
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let text = text.as_bytes();
    let text = text
        .iter()
        .position(|c| *c == b'=')
        .and_then(|end| text.get(..end))
        .unwrap_or(text);
    let mut out = Vec::with_capacity(text.len() / 4 * 3 + 3);
    let (quads, rest) = text.as_chunks::<4>();
    for &[a, b, c, d] in quads {
        let (a, b, c, d) = (value_of(a), value_of(b), value_of(c), value_of(d));
        if a == BAD || b == BAD || c == BAD || d == BAD {
            return None;
        }
        let n = u32::from(a) << 18 | u32::from(b) << 12 | u32::from(c) << 6 | u32::from(d);
        let [_, x, y, z] = n.to_be_bytes();
        out.extend_from_slice(&[x, y, z]);
    }
    let (mut acc, mut bits) = (0_u32, 0_u32);
    for &c in rest {
        let v = value_of(c);
        if v == BAD {
            return None;
        }
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits & 0xff) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc_4648_vectors() {
        for (plain, coded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(plain.as_bytes()), coded);
            assert_eq!(decode(coded).as_deref(), Some(plain.as_bytes()));
        }
        assert_eq!(
            decode("Zm9vYg").as_deref(),
            Some(&b"foob"[..]),
            "padding is optional"
        );
        assert_eq!(decode("not base64!"), None);
    }

    #[test]
    fn every_length_and_byte_round_trips_and_a_stray_character_anywhere_is_refused() {
        let all: Vec<u8> = (0..=255).cycle().take(700).collect();
        for n in 0..all.len() {
            let plain = all.get(..n).unwrap_or_default();
            let coded = encode(plain);
            assert_eq!(coded.len(), n.div_ceil(3) * 4);
            assert_eq!(decode(&coded).as_deref(), Some(plain), "length {n}");
            assert_eq!(decode(coded.trim_end_matches('=')).as_deref(), Some(plain));
        }
        let coded = encode(b"0123456789abcdef");
        for at in 0..coded.trim_end_matches('=').len() {
            let mut bad = coded.clone().into_bytes();
            if let Some(c) = bad.get_mut(at) {
                *c = b'!';
            }
            assert_eq!(
                decode(&String::from_utf8(bad).unwrap_or_default()),
                None,
                "at {at}"
            );
        }
        // Decoding stops at the first `=`, as it always has.
        assert_eq!(decode("Zg==Zm9v").as_deref(), Some(&b"f"[..]));
    }
}
