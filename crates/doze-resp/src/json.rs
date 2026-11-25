//! Just enough JSON for the value viewer, without a JSON library in the module: whether a stored string is JSON (so it
//! opens as a document, not as text), and how to write keys and members as JSON strings. A key's bytes need not be
//! UTF-8; [`shown`] writes them so a person can read them and nothing is silently dropped.

/// How deep a document may nest before it is shown as text instead (the check recurses).
const MAX_DEPTH: usize = 128;

/// Whether `b` is one whole JSON document (an object, array, string, number, `true`, `false` or `null`), with only
/// white space around it.
pub fn is_json(b: &[u8]) -> bool {
    let mut p = Parser { b, at: 0 };
    p.space();
    if !p.value(0) {
        return false;
    }
    p.space();
    p.at == b.len()
}

struct Parser<'a> {
    b: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.at).copied()
    }

    fn eat(&mut self, c: u8) -> bool {
        let hit = self.peek() == Some(c);
        if hit {
            self.at += 1;
        }
        hit
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn word(&mut self, w: &[u8]) -> bool {
        let hit = self.b.get(self.at..self.at + w.len()) == Some(w);
        if hit {
            self.at += w.len();
        }
        hit
    }

    fn value(&mut self, depth: usize) -> bool {
        if depth > MAX_DEPTH {
            return false;
        }
        match self.peek() {
            Some(b'{') => self.items(b'}', depth, true),
            Some(b'[') => self.items(b']', depth, false),
            Some(b'"') => self.string(),
            Some(b't') => self.word(b"true"),
            Some(b'f') => self.word(b"false"),
            Some(b'n') => self.word(b"null"),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => false,
        }
    }

    /// An object's members or an array's items, from its opening bracket to `close`.
    fn items(&mut self, close: u8, depth: usize, object: bool) -> bool {
        self.at += 1;
        self.space();
        if self.eat(close) {
            return true;
        }
        loop {
            if object {
                if !self.string() {
                    return false;
                }
                self.space();
                if !self.eat(b':') {
                    return false;
                }
                self.space();
            }
            if !self.value(depth + 1) {
                return false;
            }
            self.space();
            if self.eat(close) {
                return true;
            }
            if !self.eat(b',') {
                return false;
            }
            self.space();
        }
    }

    fn string(&mut self) -> bool {
        if !self.eat(b'"') {
            return false;
        }
        let start = self.at;
        loop {
            match self.peek() {
                None => return false,
                Some(b'"') => break,
                Some(b'\\') => {
                    self.at += 1;
                    match self.peek() {
                        Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => {
                            self.at += 1
                        }
                        Some(b'u') => {
                            let hex = self.b.get(self.at + 1..self.at + 5);
                            if !hex.is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit)) {
                                return false;
                            }
                            self.at += 5;
                        }
                        _ => return false,
                    }
                }
                Some(c) if c < 0x20 => return false,
                Some(_) => self.at += 1,
            }
        }
        let ok = std::str::from_utf8(self.b.get(start..self.at).unwrap_or_default()).is_ok();
        self.at += 1;
        ok
    }

    fn digits(&mut self) -> usize {
        let start = self.at;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.at += 1;
        }
        self.at - start
    }

    fn number(&mut self) -> bool {
        self.eat(b'-');
        if self.eat(b'0') {
            // No leading zeros.
        } else if self.digits() == 0 {
            return false;
        }
        if self.eat(b'.') && self.digits() == 0 {
            return false;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            let _ = self.eat(b'+') || self.eat(b'-');
            if self.digits() == 0 {
                return false;
            }
        }
        true
    }
}

/// Bytes as text a person reads: UTF-8 as it is, any other byte as `\xNN` (and a backslash as `\\`, so the two cannot
/// be confused).
pub fn shown(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) if !s.contains('\\') => s.to_string(),
        _ => {
            let mut out = String::with_capacity(b.len());
            for chunk in b.utf8_chunks() {
                out.push_str(&chunk.valid().replace('\\', "\\\\"));
                for x in chunk.invalid() {
                    out.push_str(&format!("\\x{x:02x}"));
                }
            }
            out
        }
    }
}

/// `s` as a JSON string, quotes included.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Members as a JSON array of strings.
pub fn array<'a>(items: impl IntoIterator<Item = &'a [u8]>) -> String {
    let parts: Vec<String> = items.into_iter().map(|i| quote(&shown(i))).collect();
    format!("[{}]", parts.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_told_from_text() {
        for yes in [
            r#"{"a":[1,2.5,-3e+2,true,false,null],"b":{"c":"\u00e9\n"}}"#,
            " [ ] ",
            "\"just a string\"",
            "0",
            "{}",
        ] {
            assert!(is_json(yes.as_bytes()), "{yes}");
        }
        for no in [
            "",
            "hello",
            "{",
            "{\"a\":1,}",
            "[1 2]",
            "01",
            "1.",
            "\"\\x\"",
            "{} {}",
            "{'a':1}",
            "NaN",
        ] {
            assert!(!is_json(no.as_bytes()), "{no}");
        }
        assert!(!is_json(&[b'"', 0xff, b'"']), "a JSON string is UTF-8");
        let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 5), "]".repeat(MAX_DEPTH + 5));
        assert!(
            !is_json(deep.as_bytes()),
            "too deep is text, not a stack overflow"
        );
    }

    #[test]
    fn bytes_are_shown_without_losing_any() {
        assert_eq!(shown(b"user:1"), "user:1");
        assert_eq!(shown(b"a\xffb"), "a\\xffb");
        assert_eq!(
            shown(b"a\\xffb"),
            "a\\\\xffb",
            "a literal backslash is not a byte"
        );
        assert_eq!(quote("a\"b\\\n\u{1}"), r#""a\"b\\\n\u0001""#);
        assert_eq!(array([&b"x"[..], b"y\"z"]), r#"["x","y\"z"]"#);
        assert!(is_json(array([&b"\xff\x00"[..]]).as_bytes()));
    }
}
