use super::prelude::*;

use crate::Number;

use std::str::FromStr;
use winnow::combinator::{cut_err, fail};

// PATCH (doze): number literals are lexed as Go's HCL scanner lexes them, then checked as Go's
// `big.ParseFloat` checks them. The token is a digit followed by any run of digits, `.` and
// exponents (`e`, an optional sign, a digit), not ending in `.` (scan_tokens.rl `NumberLit`); so
// `1.e5` is one number, and `1.5.2` is one invalid one, reported where it starts. The value kept
// here is a placeholder: doze-hcl reads every number from its text at 512 bits, so integers with
// hundreds of digits and floats beyond f64 are numbers too.
pub(super) fn number(input: &mut Input) -> ModalResult<Number> {
    let rest = input.peek_slice(input.eof_offset());
    let len = go_number_len(rest);
    if len == 0 {
        return fail.parse_next(input);
    }
    let checkpoint = input.checkpoint();
    let text = input.next_slice(len);
    if !go_number_valid(text) {
        input.reset(&checkpoint);
        return cut_err(fail)
            .context(StrContext::Label("number literal"))
            .context(StrContext::Expected(StrContextValue::Description(
                "digits, an optional fraction and an optional exponent",
            )))
            .parse_next(input);
    }
    Ok(match u64::from_str(text) {
        Ok(n) => Number::from(n),
        Err(_) => f64::from_str(text)
            .ok()
            .and_then(Number::from_f64)
            .unwrap_or_else(|| Number::from(0u64)),
    })
}

/// The length of Go's `NumberLit` token at the start of `s` (0 if `s` does not start with a digit).
fn go_number_len(s: &str) -> usize {
    let b = s.as_bytes();
    if !b.first().is_some_and(u8::is_ascii_digit) {
        return 0;
    }
    let mut i = 1;
    loop {
        match b.get(i) {
            Some(c) if c.is_ascii_digit() || *c == b'.' => i += 1,
            Some(b'e' | b'E') => {
                let mut j = i + 1;
                if matches!(b.get(j), Some(b'+' | b'-')) {
                    j += 1;
                }
                if b.get(j).is_some_and(u8::is_ascii_digit) {
                    i = j + 1;
                } else {
                    break;
                }
            }
            _ => break,
        }
    }
    while i > 1 && b.get(i - 1) == Some(&b'.') {
        i -= 1;
    }
    i
}

/// Whether Go's `big.ParseFloat` reads the token: digits, at most one `.` before an optional
/// exponent, an exponent with digits.
fn go_number_valid(text: &str) -> bool {
    let (mantissa, exp) = match text.find(['e', 'E']) {
        Some(i) => (&text[..i], Some(&text[i + 1..])),
        None => (text, None),
    };
    let mantissa_ok = mantissa.bytes().filter(|c| *c == b'.').count() <= 1
        && mantissa.bytes().all(|c| c.is_ascii_digit() || c == b'.');
    let exp_ok = exp.is_none_or(|e| {
        let digits = e.strip_prefix(['+', '-']).unwrap_or(e);
        !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit())
    });
    mantissa_ok && exp_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_go_number_tokens() {
        for (s, n) in [("1", 1), ("1.5", 3), ("1.e5", 4), ("1.5.2", 5), ("1.foo", 1), ("1e", 1), ("1e+5x", 4), ("12..", 2), ("x", 0)] {
            assert_eq!(go_number_len(s), n, "{s}");
        }
        assert!(go_number_valid("1.e5") && go_number_valid("10") && go_number_valid("1.5e-3"));
        assert!(!go_number_valid("1.5.2") && !go_number_valid("1e5e5") && !go_number_valid("1e5.2"));
    }
}
