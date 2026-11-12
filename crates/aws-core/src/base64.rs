//! Standard base64 (RFC 4648 §4), as the AWS JSON protocols carry binary values. Decoding takes the padding or leaves it.

const SET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `bytes` as padded base64.
#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n =
            chunk.iter().fold(0_u32, |acc, b| acc << 8 | u32::from(*b)) << (8 * (3 - chunk.len()));
        for i in 0..=chunk.len() {
            let ix = usize::try_from(n >> (18 - 6 * i) & 63).unwrap_or(0);
            out.push(char::from(SET.get(ix).copied().unwrap_or(b'A')));
        }
        out.push_str(&"=".repeat(3 - chunk.len()));
    }
    out
}

// fuzz:parser fuzz_base64
/// The bytes `text` stands for, or `None` if it is not base64.
#[must_use]
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3 + 3);
    let (mut acc, mut bits) = (0_u32, 0_u32);
    for c in text.bytes().take_while(|c| *c != b'=') {
        let v = SET.iter().position(|s| *s == c)?;
        acc = (acc << 6) | u32::try_from(v).ok()?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xff).ok()?);
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
}
