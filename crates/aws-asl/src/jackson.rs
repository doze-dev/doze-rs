//! JSON that does not parse, worded as AWS's Java service (Jackson) words it. The recording has one case, a field name that does not
//! start with a quote (`{nope`): `Unexpected character ('n' (code 110)): was expecting double-quote to start field name`, followed,
//! for a definition, by `\n at [Source: (String)"{nope"; line: 1, column: 2]`. The other sentences are Jackson's from memory.

use serde_json::Value;

/// Why `text` is not JSON: the sentence, and the line and column (1-based) where reading stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unreadable {
    pub sentence: String,
    pub line: usize,
    pub column: usize,
}

impl Unreadable {
    /// The sentence with Jackson's location, quoting the source (`(String)"..."`).
    #[must_use]
    pub fn located(&self, text: &str) -> String {
        let shown: String = text.chars().take(500).collect();
        format!(
            "{}\n at [Source: (String){}; line: {}, column: {}]",
            self.sentence,
            Value::String(shown),
            self.line,
            self.column
        )
    }
}

/// Reads `text` as one JSON value, or says why it is not one.
///
/// # Errors
/// The reason, in Jackson's words where they are known.
pub fn read(text: &str) -> Result<Value, Unreadable> {
    serde_json::from_str::<Value>(text).map_err(|e| explain(text, &e))
}

fn explain(text: &str, e: &serde_json::Error) -> Unreadable {
    let (line, column) = (e.line(), e.column());
    let at = offset(text, line, column);
    let bad = text.get(at..).and_then(|s| s.chars().next());
    let message = e.to_string();
    let sentence = match bad {
        None => "Unexpected end-of-input: expected close marker for Object".to_owned(),
        Some(c) if message.starts_with("key must be a string") => format!(
            "Unexpected character ('{c}' (code {})): was expecting double-quote to start field name",
            u32::from(c)
        ),
        Some(c)
            if (message.starts_with("expected value") || message.starts_with("expected ident"))
                && c.is_ascii_alphabetic() =>
        {
            let head = text.get(..at).unwrap_or_default();
            let start = head.len()
                - head
                    .chars()
                    .rev()
                    .take_while(char::is_ascii_alphanumeric)
                    .count();
            let word: String = text
                .get(start..)
                .unwrap_or_default()
                .chars()
                .take_while(char::is_ascii_alphanumeric)
                .collect();
            format!(
                "Unrecognized token '{word}': was expecting (JSON String, Number, Array, Object or token 'null', 'true' or 'false')"
            )
        }
        Some(c) => format!(
            "Unexpected character ('{c}' (code {})): {}",
            u32::from(c),
            expecting(&message)
        ),
    };
    Unreadable {
        sentence,
        line,
        column: column.max(1),
    }
}

fn expecting(message: &str) -> &'static str {
    if message.starts_with("expected `:`") {
        "was expecting a colon to separate field name and value"
    } else if message.starts_with("expected `,` or `}`") {
        "was expecting comma to separate Object entries"
    } else if message.starts_with("expected `,` or `]`") {
        "was expecting comma to separate Array entries"
    } else if message.starts_with("trailing characters") {
        "expected end-of-input"
    } else {
        "expected a valid value (JSON String, Number, Array, Object or token 'null', 'true' or 'false')"
    }
}

/// The byte offset of 1-based `line` and `column` (serde_json's column is that of the character it stopped on).
fn offset(text: &str, line: usize, column: usize) -> usize {
    let start: usize = text
        .split_inclusive('\n')
        .take(line.saturating_sub(1))
        .map(str::len)
        .sum();
    let rest = text.get(start..).unwrap_or_default();
    start
        + rest
            .char_indices()
            .nth(column.saturating_sub(1))
            .map_or(rest.len(), |(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_name_without_a_quote_is_worded_as_recorded() {
        let e = read("{nope").unwrap_err();
        assert_eq!(
            e.sentence,
            "Unexpected character ('n' (code 110)): was expecting double-quote to start field name"
        );
        assert_eq!(
            e.located("{nope"),
            "Unexpected character ('n' (code 110)): was expecting double-quote to start field name\n at [Source: (String)\"{nope\"; line: 1, column: 2]"
        );
        assert!(
            read("nope")
                .unwrap_err()
                .sentence
                .starts_with("Unrecognized token 'nope'")
        );
        assert!(read("{\"a\":1").is_err());
    }
}
