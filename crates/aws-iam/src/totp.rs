//! Time-based one-time passwords as a virtual MFA device makes them (RFC 6238 over RFC 4226: HMAC-SHA-1, 30-second steps, six
//! digits), and the base32 text (RFC 4648, no padding) its seed is shown in.

use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;

/// Seconds a code lasts.
pub(crate) const STEP_SECS: i64 = 30;

/// The step a time falls in.
pub(crate) const fn step_of(now_ms: i64) -> i64 {
    now_ms.div_euclid(STEP_SECS * 1000)
}

/// The six-digit code for `step`.
pub(crate) fn code(seed: &[u8], step: i64) -> Option<String> {
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(seed).ok()?;
    mac.update(&step.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest.last()? & 0x0f);
    let word: [u8; 4] = digest.get(offset..offset + 4)?.try_into().ok()?;
    let n = u32::from_be_bytes(word) & 0x7fff_ffff;
    Some(format!("{:06}", n % 1_000_000))
}

/// The first step in `steps` whose code is `first` and whose next step's code is `second`: how a device is enabled or brought back in
/// step (two consecutive codes).
pub(crate) fn consecutive(
    seed: &[u8],
    steps: std::ops::RangeInclusive<i64>,
    first: &str,
    second: &str,
) -> Option<i64> {
    steps.into_iter().find(|&s| {
        code(seed, s).as_deref() == Some(first) && code(seed, s + 1).as_deref() == Some(second)
    })
}

/// The step in `steps` whose code is `given`.
pub(crate) fn matching(
    seed: &[u8],
    steps: std::ops::RangeInclusive<i64>,
    given: &str,
) -> Option<i64> {
    steps
        .into_iter()
        .find(|&s| code(seed, s).as_deref() == Some(given))
}

/// Base32 (RFC 4648 alphabet) without padding, as an authenticator app takes a seed.
pub(crate) fn base32(bytes: &[u8]) -> String {
    const SET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let (mut buffer, mut bits) = (0_u32, 0_u32);
    for b in bytes {
        buffer = (buffer << 8) | u32::from(*b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(letter(SET, buffer >> bits));
        }
    }
    if bits > 0 {
        out.push(letter(SET, buffer << (5 - bits)));
    }
    out
}

fn letter(set: &[u8; 32], v: u32) -> char {
    let ix = usize::try_from(v & 31).unwrap_or_default();
    char::from(set.get(ix).copied().unwrap_or(b'A'))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// RFC 6238 appendix B: the SHA-1 seed `12345678901234567890`, eight digits there; the last six are the six-digit code.
    #[test]
    fn the_codes_are_the_rfcs() {
        let seed = b"12345678901234567890";
        for (secs, eight) in [
            (59_i64, "94287082"),
            (1_111_111_109, "07081804"),
            (1_234_567_890, "89005924"),
            (2_000_000_000, "69279037"),
        ] {
            let step = step_of(secs * 1000);
            assert_eq!(code(seed, step).unwrap(), eight[2..]);
        }
    }

    #[test]
    fn two_consecutive_codes_find_their_step_and_one_code_does_not_repeat_elsewhere() {
        let seed = b"12345678901234567890";
        let (a, b) = (code(seed, 100).unwrap(), code(seed, 101).unwrap());
        assert_eq!(consecutive(seed, 90..=110, &a, &b), Some(100));
        assert_eq!(consecutive(seed, 90..=110, &b, &a), None);
        assert_eq!(matching(seed, 99..=101, &b), Some(101));
        assert_eq!(matching(seed, 102..=104, &b), None);
    }

    #[test]
    fn base32_is_the_rfcs() {
        assert_eq!(base32(b""), "");
        assert_eq!(base32(b"f"), "MY");
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
        assert_eq!(base32(&[0xff; 20]).len(), 32);
    }
}
