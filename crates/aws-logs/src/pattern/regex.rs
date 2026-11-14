//! Regular expressions in filter patterns: `%...%`, alone among the terms of an unstructured pattern (`%ERROR|WARN%`), as the value of
//! a JSON comparison (`{ $.path = %^/api/v[12]/% }`, `=` and `!=`) or of a space-delimited field (`[ip, method = %GET|PUT%, ...]`).
//!
//! The rules, from the CloudWatch Logs model's documentation (`PutMetricFilter`, `PutSubscriptionFilter`): at most two regular
//! expressions in one filter pattern, and five across the metric and subscription filters of one log group. From the Logs User
//! Guide's "regular expressions in filter patterns", which is (memory): an expression is at most 25 characters; it searches (it is
//! not anchored unless it says `^` or `$`); the syntax is a limited one: characters, `.`, `*`, `+`, `?`, `|`, `( )`, `[ ]` with
//! ranges and `^`, `{m,n}`, `^`, `$`, the classes `\d \D \s \S \w \W`, and `\` before a symbol. Lookaround, back references, flags,
//! named groups, `\b` and Unicode classes are refused as an invalid pattern.

use super::PatternError;

/// The longest expression between the `%` signs. (memory)
pub(crate) const MAX_LEN: usize = 25;
/// The most expressions in one filter pattern (the model's documentation).
pub(crate) const MAX_PER_PATTERN: usize = 2;
/// The most expressions across one log group's filters (the model's documentation).
pub(crate) const MAX_PER_GROUP: usize = 5;

/// A compiled `%...%`.
#[derive(Clone, Debug)]
pub(crate) struct Re(regex_lite::Regex);

impl PartialEq for Re {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_str() == other.0.as_str()
    }
}

impl Re {
    /// Compiles the text between the `%` signs, within AWS's limits.
    ///
    /// # Errors
    /// [`PatternError::Invalid`] for an empty or too long expression, or one outside the supported syntax.
    pub(crate) fn new(body: &str) -> Result<Self, PatternError> {
        if body.is_empty() || body.chars().count() > MAX_LEN || !supported(body) {
            return Err(PatternError::Invalid);
        }
        regex_lite::Regex::new(body)
            .map(Self)
            .map_err(|_| PatternError::Invalid)
    }

    /// Whether the expression is found in `text`.
    pub(crate) fn found(&self, text: &str) -> bool {
        self.0.is_match(text)
    }
}

/// Whether `body` keeps to the documented syntax: no group modifier (`(?`), and after `\` only a class letter or a symbol.
fn supported(body: &str) -> bool {
    if body.contains("(?") {
        return false;
    }
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('d' | 'D' | 's' | 'S' | 'w' | 'W') => {}
                Some(x) if !x.is_alphanumeric() => {}
                _ => return false,
            }
        }
    }
    true
}

/// Reads a `%...%` whose opening `%` has been consumed, up to the closing one (a `\%` is a `%` in the expression).
pub(crate) fn read(chars: &mut impl Iterator<Item = char>) -> Result<Re, PatternError> {
    let mut body = String::new();
    loop {
        match chars.next() {
            Some('%') => return Re::new(&body),
            Some('\\') => {
                let next = chars.next().ok_or(PatternError::Invalid)?;
                if next != '%' {
                    body.push('\\');
                }
                body.push(next);
            }
            Some(c) => body.push(c),
            None => return Err(PatternError::Invalid),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_documented_syntax_compiles_and_the_rest_is_refused() {
        for ok in [
            r"ERROR|WARN",
            r"^/api/v[12]/",
            r"\d{3}",
            r"a.+b?",
            r"\.json$",
            r"[^a-z]\s\w",
        ] {
            assert!(Re::new(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "(?i)error",
            "(?=x)",
            r"\bword",
            r"\1",
            r"\p{L}",
            "abcdefghijklmnopqrstuvwxyz",
            "[unclosed",
        ] {
            assert_eq!(Re::new(bad), Err(PatternError::Invalid), "{bad}");
        }
    }

    #[test]
    fn an_expression_searches_and_is_read_up_to_its_closing_sign() {
        let re = Re::new("ERR|WARN").unwrap();
        assert!(re.found("2026 WARN disk") && !re.found("info"));
        let mut rest = r"50\%|x% tail".chars();
        let re = read(&mut rest).unwrap();
        assert!(re.found("at 50%"));
        assert_eq!(rest.collect::<String>(), " tail");
        assert!(read(&mut "open".chars()).is_err());
    }
}
