//! Lower-case hex, no dependency.

/// Encode `bytes` as lower-case hex.
#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for b in bytes {
        out.push(nibble_char(b >> 4));
        out.push(nibble_char(b & 0x0f));
    }
    out
}

const fn nibble_char(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        _ => (b'a' + (n - 10)) as char,
    }
}

/// Value of one hex digit (either case).
#[must_use]
pub const fn digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Decode hex of any even length; `None` on any non-hex byte or odd length.
#[must_use]
pub fn decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 2);
    let mut it = s.bytes();
    while let Some(hi) = it.next() {
        let lo = it.next()?;
        out.push(digit(hi)? << 4 | digit(lo)?);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    #[test]
    fn round_trip() {
        assert_eq!(encode(&[0, 0xab, 0xff]), "00abff");
        assert_eq!(decode("00ABff").unwrap(), vec![0, 0xab, 0xff]);
        assert!(decode("abc").is_none());
        assert!(decode("zz").is_none());
    }
}
