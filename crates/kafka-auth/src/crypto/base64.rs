//! Standard base64 (RFC 4648 section 4): the alphabet `A-Za-z0-9+/` and `=` padding, nothing else.
//!
//! Decoding is strict. Text that is not a multiple of four characters, that has a character outside the
//! alphabet (white space included), padding anywhere but the end, or bits set past the end of the data (a
//! second encoding of the same bytes) is refused.

/// Text that is not base64 in the strict sense of this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not valid base64")]
pub struct Invalid;

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `data` as base64, with padding.
#[must_use]
pub fn encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        // A chunk of one byte fills two characters, of two bytes three, of three bytes four.
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                char::from(symbol(n >> (18 - 6 * i)))
            } else {
                '='
            });
        }
    }
    out
}

fn symbol(six_bits: u32) -> u8 {
    let at = usize::try_from(six_bits & 63).unwrap_or(0);
    ALPHABET.get(at).copied().unwrap_or(b'=')
}

/// The bytes `text` stands for.
///
/// # Errors
/// [`Invalid`] for anything but canonical, padded base64.
pub fn decode(text: &[u8]) -> Result<Vec<u8>, Invalid> {
    decode_with(text, true)
}

/// The bytes `text` stands for, as Java's `Base64.getDecoder()` reads it: the shape is as strict as in
/// [`decode`], but bits set past the end of the data are ignored, so a second encoding of the same bytes is
/// accepted. Apache Kafka's SCRAM messages are decoded this way, and a broker that means to answer as Kafka does
/// must not be stricter.
///
/// # Errors
/// [`Invalid`] for anything but padded base64.
pub fn decode_lax(text: &[u8]) -> Result<Vec<u8>, Invalid> {
    decode_with(text, false)
}

fn decode_with(text: &[u8], canonical: bool) -> Result<Vec<u8>, Invalid> {
    if !text.len().is_multiple_of(4) {
        return Err(Invalid);
    }
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let last_at = text.len().saturating_sub(4);
    let (quads, _) = text.as_chunks::<4>();
    for (i, quad) in quads.iter().enumerate() {
        quartet(quad, (i * 4 == last_at, canonical), &mut out)?;
    }
    Ok(out)
}

/// Four characters to up to three bytes, appended to `out`. Only the last four characters may be padded.
/// `(last, canonical)`: whether this is the last four characters, and whether spare bits are refused.
fn quartet(
    &[a, b, c, d]: &[u8; 4],
    (last, canonical): (bool, bool),
    out: &mut Vec<u8>,
) -> Result<(), Invalid> {
    let pad = match (c == b'=', d == b'=') {
        (false, false) => 0,
        (false, true) => 1,
        (true, true) => 2,
        (true, false) => return Err(Invalid),
    };
    if pad > 0 && !last {
        return Err(Invalid);
    }
    let sextet = |ch: u8, wanted: bool| if wanted { value(ch) } else { Ok(0) };
    let n = sextet(a, true)? << 18
        | sextet(b, true)? << 12
        | sextet(c, pad < 2)? << 6
        | sextet(d, pad < 1)?;
    // Bits past the data must be zero: the canonical form is the only one accepted.
    let spare = match pad {
        0 => 0,
        1 => n & 0xFF,
        _ => n & 0xFFFF,
    };
    if canonical && spare != 0 {
        return Err(Invalid);
    }
    out.push(byte(n >> 16));
    if pad < 2 {
        out.push(byte(n >> 8));
    }
    if pad < 1 {
        out.push(byte(n));
    }
    Ok(())
}

fn byte(n: u32) -> u8 {
    u8::try_from(n & 0xFF).unwrap_or(0)
}

fn value(ch: u8) -> Result<u32, Invalid> {
    match ch {
        b'A'..=b'Z' => Ok(u32::from(ch - b'A')),
        b'a'..=b'z' => Ok(u32::from(ch - b'a') + 26),
        b'0'..=b'9' => Ok(u32::from(ch - b'0') + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(Invalid),
    }
}

#[cfg(test)]
mod tests;
