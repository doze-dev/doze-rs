//! The part of TOML `doze-aws.toml` uses: `key = value` lines, comments, and values that are strings (basic or literal), integers,
//! booleans, or arrays of those (on one line or several). Anything else (a table header, a dotted key, a date, an inline table) is
//! refused with its line number rather than half-read. Written here rather than taken from a crate: the file is flat by design,
//! and a TOML crate is a few hundred kilobytes of binary for it.

use std::collections::BTreeMap;

/// A value as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Value {
    /// A string, an integer or a boolean, as its text (`"a"` is `a`, `true` is `true`).
    One(String),
    /// An array.
    Many(Vec<String>),
}

/// A value and the line it is on.
pub(crate) type Entries = BTreeMap<String, (Value, usize)>;

/// Reads `text`, or says which line is wrong and why.
pub(crate) fn parse(text: &str) -> Result<Entries, String> {
    let mut out = Entries::new();
    let mut lines = text.lines().enumerate().map(|(i, l)| (i + 1, l));
    while let Some((n, line)) = lines.next() {
        let line = strip_comment(line).trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            return Err(format!(
                "line {n}: tables are not used in doze-aws.toml; every key is at the top level"
            ));
        }
        let (key, rest) = line
            .split_once('=')
            .ok_or_else(|| format!("line {n}: expected `key = value`"))?;
        let key = key.trim();
        if key.is_empty()
            || !key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(format!("line {n}: `{key}` is not a key doze-aws.toml uses"));
        }
        let mut rest = rest.trim().to_owned();
        // An array may run over several lines: read until its brackets balance.
        while rest.starts_with('[') && !balanced(&rest) {
            let (_, more) = lines
                .next()
                .ok_or_else(|| format!("line {n}: the array is not closed"))?;
            rest.push(' ');
            rest.push_str(strip_comment(more).trim());
        }
        let value = value(&rest).map_err(|e| format!("line {n}: {e}"))?;
        if out.insert(key.to_owned(), (value, n)).is_some() {
            return Err(format!("line {n}: `{key}` is set twice"));
        }
    }
    Ok(out)
}

/// The line without a `#` comment (a `#` inside a string is kept).
fn strip_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        match (quote, c) {
            (Some('"'), '\\') if !escaped => {
                escaped = true;
                continue;
            }
            (Some(q), c) if c == q && !escaped => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, '#') => return line.get(..i).unwrap_or(line),
            _ => {}
        }
        escaped = false;
    }
    line
}

fn balanced(s: &str) -> bool {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for c in s.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, '[') => depth += 1,
            (None, ']') => depth -= 1,
            _ => {}
        }
    }
    depth <= 0
}

fn value(s: &str) -> Result<Value, String> {
    if let Some(inner) = s.strip_prefix('[') {
        let inner = inner
            .strip_suffix(']')
            .ok_or("an array must end with `]`")?;
        let mut items = Vec::new();
        for item in split_items(inner)? {
            let item = item.trim();
            if !item.is_empty() {
                items.push(scalar(item)?);
            }
        }
        return Ok(Value::Many(items));
    }
    scalar(s).map(Value::One)
}

/// The items of an array body, split at commas outside strings.
fn split_items(s: &str) -> Result<Vec<&str>, String> {
    let mut out = Vec::new();
    let (mut start, mut quote) = (0, None::<char>);
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, '[' | '{') => return Err("arrays hold strings, numbers or booleans".into()),
            (None, ',') => {
                out.push(s.get(start..i).unwrap_or_default());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(s.get(start..).unwrap_or_default());
    Ok(out)
}

fn scalar(s: &str) -> Result<String, String> {
    if let Some(body) = s.strip_prefix('"') {
        return basic(body.strip_suffix('"').ok_or("the string is not closed")?);
    }
    if let Some(body) = s.strip_prefix('\'') {
        return body
            .strip_suffix('\'')
            .map(str::to_owned)
            .ok_or_else(|| "the string is not closed".into());
    }
    let plain = s.replace('_', "");
    if s == "true" || s == "false" || (!plain.is_empty() && plain.parse::<i64>().is_ok()) {
        return Ok(plain);
    }
    if s.starts_with('{') {
        return Err("inline tables are not used in doze-aws.toml".into());
    }
    Err(format!("`{s}` is not a value: quote a string (\"{s}\")"))
}

/// A basic string's body with its escapes resolved.
fn basic(body: &str) -> Result<String, String> {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                let ch = u32::from_str_radix(&hex, 16)
                    .ok()
                    .and_then(char::from_u32)
                    .ok_or("a \\u escape needs four hex digits")?;
                out.push(ch);
            }
            other => {
                return Err(format!(
                    "`\\{}` is not an escape TOML knows",
                    other.unwrap_or(' ')
                ));
            }
        }
    }
    Ok(out)
}

/// `s` as a TOML basic string.
pub(crate) fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(s: &str) -> Value {
        Value::One(s.to_owned())
    }

    #[test]
    fn keys_values_comments_and_arrays_are_read() {
        let text = "# top\nlisten = \"127.0.0.1:4566\" # where\nidle-secs = 1_000\nlambda-quiet = true\n\
                    services = [\n  \"sqs\", # queues\n  's3',\n]\nname = \"a#b\\\"c\"\n";
        let got = parse(text).unwrap();
        assert_eq!(got["listen"], (one("127.0.0.1:4566"), 2));
        assert_eq!(got["idle-secs"].0, one("1000"));
        assert_eq!(got["lambda-quiet"].0, one("true"));
        assert_eq!(
            got["services"].0,
            Value::Many(vec!["sqs".into(), "s3".into()])
        );
        assert_eq!(got["name"].0, one("a#b\"c"));
    }

    #[test]
    fn what_the_file_does_not_use_is_refused_by_line() {
        for (text, needle) in [
            ("[lambda]\n", "line 1: tables"),
            (
                "\nregion = us-east-1\n",
                "line 2: `us-east-1` is not a value",
            ),
            ("a = 1\na = 2\n", "line 2: `a` is set twice"),
            ("a = [\"x\"\n", "not closed"),
            ("a.b = 1\n", "not a key"),
            ("a = { x = 1 }\n", "inline tables"),
        ] {
            let e = parse(text).unwrap_err();
            assert!(e.contains(needle), "{text:?}: {e}");
        }
    }

    #[test]
    fn quote_round_trips() {
        let s = "a \"b\" \\ c";
        assert_eq!(
            parse(&format!("k = {}\n", quote(s))).unwrap()["k"].0,
            one(s)
        );
    }
}
