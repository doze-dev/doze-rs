//! JSON text to cty values, as cty's `json.ImpliedType` and `json.Unmarshal` read it through Go's
//! `encoding/json` decoder: one strict JSON value with full-precision numbers, then only white space
//! or a closing bracket (the decoder's `More` stops at those); arrays become tuples, objects
//! objects, `null` the dynamic null.

use crate::number::Number;
use crate::types::Type;
use crate::value::Value;
use std::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;

/// The deepest nesting read. Go's decoder allows 10,000 levels; doze stops earlier so that every
/// recursive walk over the value stays well inside a thread's stack.
const MAX_DEPTH: usize = 512;

pub(super) fn decode(text: &str) -> Result<Value, String> {
    let mut p = Parser {
        b: text.as_bytes(),
        s: text,
        at: 0,
    };
    p.space();
    if p.at >= p.b.len() {
        return Err("EOF".to_owned());
    }
    let v = p.value(0)?;
    p.space();
    if p.b.get(p.at).is_some_and(|c| *c != b']' && *c != b'}') {
        return Err("extraneous data after JSON object".to_owned());
    }
    Ok(v)
}

struct Parser<'a> {
    b: &'a [u8],
    s: &'a str,
    at: usize,
}

impl Parser<'_> {
    fn space(&mut self) {
        while self
            .b
            .get(self.at)
            .is_some_and(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r'))
        {
            self.at += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.at).copied()
    }

    fn unexpected(&self, context: &str) -> String {
        match self.s.get(self.at..).and_then(|r| r.chars().next()) {
            Some(c) => format!("invalid character {} {context}", go_char(c)),
            None => "unexpected EOF".to_owned(),
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth >= MAX_DEPTH {
            return Err("exceeded max depth".to_owned());
        }
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => self.string().map(|s| Value::string(&s)),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'n') => self.literal("null", Value::Null(Type::Dynamic)),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.unexpected("looking for beginning of value")),
        }
    }

    fn literal(&mut self, word: &str, v: Value) -> Result<Value, String> {
        for want in word.bytes() {
            if self.peek() != Some(want) {
                return Err(self.unexpected(&format!(
                    "in literal {word} (expecting {})",
                    go_char(char::from(want))
                )));
            }
            self.at += 1;
        }
        Ok(v)
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        match self.peek() {
            Some(b'0') => self.at += 1,
            Some(b'1'..=b'9') => self.digits(),
            _ => return Err(self.unexpected("in numeric literal")),
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            if !self.peek().is_some_and(|c| c.is_ascii_digit()) {
                return Err(self.unexpected("after decimal point in numeric literal"));
            }
            self.digits();
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if !self.peek().is_some_and(|c| c.is_ascii_digit()) {
                return Err(self.unexpected("in exponent of numeric literal"));
            }
            self.digits();
        }
        let text = self.s.get(start..self.at).unwrap_or("");
        Number::parse(text)
            .map(Value::Number)
            .ok_or_else(|| "a number is required".to_owned())
    }

    fn digits(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.at += 1;
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.at += 1;
        let mut out = String::new();
        loop {
            let rest = self.s.get(self.at..).unwrap_or("");
            let Some(c) = rest.chars().next() else {
                return Err("unexpected EOF".to_owned());
            };
            self.at += c.len_utf8();
            match c {
                '"' => return Ok(out),
                '\\' => self.escape(&mut out)?,
                c if u32::from(c) < 0x20 => {
                    return Err(format!(
                        "invalid character {} in string literal",
                        go_char(c)
                    ));
                }
                c => out.push(c),
            }
        }
    }

    fn escape(&mut self, out: &mut String) -> Result<(), String> {
        let c = self.peek();
        self.at += 1;
        let simple = match c {
            Some(b'"') => '"',
            Some(b'\\') => '\\',
            Some(b'/') => '/',
            Some(b'b') => '\u{8}',
            Some(b'f') => '\u{c}',
            Some(b'n') => '\n',
            Some(b'r') => '\r',
            Some(b't') => '\t',
            Some(b'u') => return self.unicode_escape(out),
            _ => {
                self.at -= 1;
                return Err(self.unexpected("in string escape code"));
            }
        };
        out.push(simple);
        Ok(())
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let mut v = 0;
        for _ in 0..4 {
            let d = self.peek().and_then(|c| char::from(c).to_digit(16));
            let Some(d) = d else {
                return Err(self.unexpected("in \\u hexadecimal character escape"));
            };
            v = v * 16 + d;
            self.at += 1;
        }
        Ok(v)
    }

    /// `\uXXXX`, joining a surrogate pair; a surrogate without its partner is U+FFFD.
    fn unicode_escape(&mut self, out: &mut String) -> Result<(), String> {
        let hi = self.hex4()?;
        if (0xD800..0xDC00).contains(&hi)
            && self.b.get(self.at) == Some(&b'\\')
            && self.b.get(self.at + 1) == Some(&b'u')
        {
            let save = self.at;
            self.at += 2;
            if let Ok(lo) = self.hex4()
                && (0xDC00..0xE000).contains(&lo)
            {
                let c = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                out.push(char::from_u32(c).unwrap_or('\u{fffd}'));
                return Ok(());
            }
            self.at = save;
        }
        out.push(char::from_u32(hi).unwrap_or('\u{fffd}'));
        Ok(())
    }

    fn array(&mut self, depth: usize) -> Result<Value, String> {
        self.at += 1;
        let mut out = Vec::new();
        self.space();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Value::Tuple(out));
        }
        loop {
            self.space();
            out.push(self.value(depth + 1)?);
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Value::Tuple(out));
                }
                _ => return Err(self.unexpected("after array element")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, String> {
        self.at += 1;
        let mut out: BTreeMap<String, Value> = BTreeMap::new();
        self.space();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Value::Object(out));
        }
        loop {
            self.space();
            if self.peek() != Some(b'"') {
                return Err(self.unexpected("looking for beginning of object key string"));
            }
            let key = self.string()?;
            self.space();
            if self.peek() != Some(b':') {
                return Err(self.unexpected("after object key"));
            }
            self.at += 1;
            self.space();
            let v = self.value(depth + 1)?;
            add_attr(&mut out, key, v)?;
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Value::Object(out));
                }
                _ => return Err(self.unexpected("after object key:value pair")),
            }
        }
    }
}

/// A repeated key must keep its type (the later value wins); a key that is not NFC is not found
/// among the attributes cty normalized, so it is refused.
fn add_attr(out: &mut BTreeMap<String, Value>, key: String, v: Value) -> Result<(), String> {
    let nfc: String = key.nfc().collect();
    if let Some(prev) = out.get(&nfc)
        && prev.ty() != v.ty()
    {
        return Err(format!(
            "duplicate {} property in JSON object",
            crate::value::quote::go_quote(&key)
        ));
    }
    if nfc != key {
        return Err(format!(
            "unsupported attribute {}",
            crate::value::quote::go_quote(&key)
        ));
    }
    out.insert(nfc, v);
    Ok(())
}

/// Go's `quoteChar`, as its JSON syntax errors print a character.
fn go_char(c: char) -> String {
    match c {
        '\'' => "'\\''".to_owned(),
        '"' => "'\"'".to_owned(),
        c => {
            let q = crate::value::quote::go_quote(c.encode_utf8(&mut [0; 4]));
            format!("'{}'", q.get(1..q.len().saturating_sub(1)).unwrap_or(""))
        }
    }
}
