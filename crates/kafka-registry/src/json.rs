//! JSON that keeps the order of an object's keys and the text of its numbers: what a schema language built on JSON
//! (Avro, JSON Schema) needs to store a schema the way its writer wrote it. A schema's stored text is written from this,
//! with the escapes Confluent's JSON library (Jackson) uses, so the same schema sent twice is the same text.
//!
//! The parser is strict RFC 8259 and bounded in depth: the registry refuses text nested past
//! [`MAX_SCHEMA_DEPTH`] before any parser sees it, and the parser's own bound only keeps the stack safe.

use std::fmt::Write as _;

/// How deeply the JSON of an Avro or JSON Schema schema may nest. Comparing two schemas costs more than linear time in
/// their depth; 256 levels (128 nested Avro arrays) take a few milliseconds.
pub const MAX_SCHEMA_DEPTH: usize = 256;

/// The parser's own bound on nesting: past it the text is refused, whatever the caller checked.
const PARSER_DEPTH: usize = 1024;

/// A JSON value. Objects keep their keys in order, repeats included; numbers and literals keep their text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Json {
    Object(Vec<(String, Json)>),
    Array(Vec<Json>),
    Str(String),
    /// A number, `true`, `false` or `null`, as written.
    Raw(String),
}

/// Why text is not JSON.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{what} at byte {at}")]
pub struct JsonError {
    what: &'static str,
    at: usize,
}

impl Json {
    /// Reads one JSON value, with nothing but whitespace after it.
    ///
    /// # Errors
    /// The text is not JSON, or nests past the parser's bound.
    // fuzz:parser json_node
    pub fn parse(text: &str) -> Result<Self, JsonError> {
        let mut p = Parser {
            b: text.as_bytes(),
            at: 0,
        };
        let v = p.value(0)?;
        p.ws();
        if p.at < p.b.len() {
            return Err(p.fail("unexpected content after the value"));
        }
        Ok(v)
    }

    /// The value of the first `key` of an object.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The string this is, if it is one.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Sets `key` (its first occurrence) or adds it at the end.
    pub fn set(&mut self, key: &str, value: Self) {
        if let Self::Object(m) = self {
            match m.iter_mut().find(|(k, _)| k == key) {
                Some((_, v)) => *v = value,
                None => m.push((key.to_owned(), value)),
            }
        }
    }

    /// Removes the first `key` of an object.
    pub fn remove(&mut self, key: &str) {
        if let Self::Object(m) = self
            && let Some(i) = m.iter().position(|(k, _)| k == key)
        {
            m.remove(i);
        }
    }

    /// Writes the value compactly.
    pub fn write(&self, out: &mut String) {
        match self {
            Self::Str(s) => write_str(out, s),
            Self::Raw(r) => out.push_str(r),
            Self::Array(items) => {
                out.push('[');
                for (i, it) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    it.write(out);
                }
                out.push(']');
            }
            Self::Object(m) => {
                out.push('{');
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_str(out, k);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }

    /// The value written compactly.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }
}

/// Writes `s` as a JSON string with Jackson's escapes: the quote, the backslash, and the control characters, the
/// common five by their letters and the rest as `\u00XX`. Everything else, `<` and non-ASCII included, is itself.
pub fn write_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04X}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The deepest nesting of objects and arrays in `s`, outside strings. A scan, not a parse: text that is not JSON is left
/// for the parser to refuse.
#[must_use]
pub fn depth(s: &str) -> usize {
    let (mut depth, mut deepest, mut in_string, mut escaped) = (0_usize, 0_usize, false, false);
    for c in s.bytes() {
        match c {
            _ if in_string && escaped => escaped = false,
            b'\\' if in_string => escaped = true,
            b'"' => in_string = !in_string,
            _ if in_string => {}
            b'{' | b'[' => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    deepest
}

struct Parser<'a> {
    b: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn fail(&self, what: &'static str) -> JsonError {
        JsonError { what, at: self.at }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.at).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        if self.peek() == Some(c) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, JsonError> {
        if depth > PARSER_DEPTH {
            return Err(self.fail("nested too deeply"));
        }
        self.ws();
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => self.string().map(Json::Str),
            Some(b't') => self.literal("true"),
            Some(b'f') => self.literal("false"),
            Some(b'n') => self.literal("null"),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.fail("expected a value")),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.at += 1;
        let mut m = Vec::new();
        if self.eat(b'}') {
            return Ok(Json::Object(m));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(self.fail("expected a key"));
            }
            let k = self.string()?;
            if !self.eat(b':') {
                return Err(self.fail("expected ':'"));
            }
            m.push((k, self.value(depth + 1)?));
            if self.eat(b',') {
                continue;
            }
            if self.eat(b'}') {
                return Ok(Json::Object(m));
            }
            return Err(self.fail("expected ',' or '}'"));
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.at += 1;
        let mut items = Vec::new();
        if self.eat(b']') {
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            if self.eat(b',') {
                continue;
            }
            if self.eat(b']') {
                return Ok(Json::Array(items));
            }
            return Err(self.fail("expected ',' or ']'"));
        }
    }

    fn literal(&mut self, word: &'static str) -> Result<Json, JsonError> {
        let end = self.at + word.len();
        if self.b.get(self.at..end) == Some(word.as_bytes()) {
            self.at = end;
            Ok(Json::Raw(word.to_owned()))
        } else {
            Err(self.fail("expected a literal"))
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.at;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        self.at - start
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        match self.peek() {
            Some(b'0') => self.at += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(self.fail("expected a digit")),
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            if self.digits() == 0 {
                return Err(self.fail("expected a digit after '.'"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if self.digits() == 0 {
                return Err(self.fail("expected an exponent"));
            }
        }
        let text = self.b.get(start..self.at).unwrap_or_default();
        Ok(Json::Raw(String::from_utf8_lossy(text).into_owned()))
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let mut v = 0_u32;
        for _ in 0..4 {
            let d = self
                .peek()
                .and_then(|c| char::from(c).to_digit(16))
                .ok_or_else(|| self.fail("expected four hex digits"))?;
            v = v * 16 + d;
            self.at += 1;
        }
        Ok(v)
    }

    fn escape(&mut self, out: &mut String) -> Result<(), JsonError> {
        let c = self
            .peek()
            .ok_or_else(|| self.fail("unterminated escape"))?;
        self.at += 1;
        let simple = match c {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => return self.unicode(out),
            _ => return Err(self.fail("unknown escape")),
        };
        out.push(simple);
        Ok(())
    }

    /// A `\u` escape; a surrogate pair is one character, a lone surrogate is U+FFFD (as Go's decoder reads it).
    fn unicode(&mut self, out: &mut String) -> Result<(), JsonError> {
        let hi = self.hex4()?;
        if (0xD800..0xDC00).contains(&hi) && self.b.get(self.at..self.at + 2) == Some(b"\\u") {
            let save = self.at;
            self.at += 2;
            let lo = self.hex4()?;
            if (0xDC00..0xE000).contains(&lo) {
                let c = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                out.push(char::from_u32(c).unwrap_or('\u{FFFD}'));
                return Ok(());
            }
            self.at = save;
        }
        out.push(char::from_u32(hi).unwrap_or('\u{FFFD}'));
        Ok(())
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.at += 1;
        let mut out = String::new();
        loop {
            let start = self.at;
            while matches!(self.peek(), Some(c) if c != b'"' && c != b'\\' && c >= 0x20) {
                self.at += 1;
            }
            let run = self.b.get(start..self.at).unwrap_or_default();
            out.push_str(&String::from_utf8_lossy(run));
            match self.peek() {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.at += 1;
                    self.escape(&mut out)?;
                }
                Some(_) => return Err(self.fail("a control character in a string")),
                None => return Err(self.fail("unterminated string")),
            }
        }
    }
}

#[cfg(test)]
mod tests;
