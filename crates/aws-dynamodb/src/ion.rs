//! Amazon Ion text, as far as DynamoDB exports write it and imports read it (the developer guide's mapping): a string is an Ion
//! string, a number an Ion decimal (`103.`, `6d2`), binary a blob (`{{...}}`), a boolean and null as themselves, a list an Ion list,
//! a map a struct, and a set a list annotated `$dynamodb_SS`, `$dynamodb_NS` or `$dynamodb_BS`. The reader takes the rest of the
//! text form a hand-written file may use (integers, floats, symbols, quoted field names, comments, typed nulls) and refuses what has
//! no DynamoDB meaning (timestamps, clobs, s-expressions). It is bounded: nesting deeper than an item may be is refused.

use std::collections::{BTreeMap, BTreeSet};

use aws_expr::{MAX_DEPTH, Number, Value};

/// The version marker each line of an export starts with.
pub(crate) const MARKER: &str = "$ion_1_0";

/// One line of an export: `$ion_1_0 {<wrapper>:<value>}` (`Item` for a full export, `Record` for an incremental one).
pub(crate) fn line(wrapper: &str, value: &Value) -> String {
    let mut out = String::from(MARKER);
    out.push_str(" {");
    out.push_str(wrapper);
    out.push(':');
    write(value, &mut out);
    out.push('}');
    out
}

fn field_name(k: &str, out: &mut String) {
    let ident = k
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        && !matches!(k, "null" | "true" | "false" | "nan");
    if ident {
        out.push_str(k);
    } else {
        string(k, out);
    }
}

fn string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\x{:02x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A number as an Ion decimal: an exponent is written with `d`, and a whole number ends in `.`.
fn decimal(n: &Number, out: &mut String) {
    let text = n.to_string();
    if let Some((m, e)) = text.split_once('E') {
        out.push_str(m);
        out.push('d');
        out.push_str(e.trim_start_matches('+'));
    } else if text.contains('.') {
        out.push_str(&text);
    } else {
        out.push_str(&text);
        out.push('.');
    }
}

fn blob(b: &[u8], out: &mut String) {
    out.push_str("{{");
    out.push_str(&aws_core::base64::encode(b));
    out.push_str("}}");
}

fn list<T>(
    items: impl IntoIterator<Item = T>,
    out: &mut String,
    mut each: impl FnMut(T, &mut String),
) {
    out.push('[');
    for (i, v) in items.into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        each(v, out);
    }
    out.push(']');
}

/// A value in Ion text.
pub(crate) fn write(v: &Value, out: &mut String) {
    match v {
        Value::S(s) => string(s, out),
        Value::N(n) => decimal(n, out),
        Value::B(b) => blob(b, out),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Null => out.push_str("null"),
        Value::Ss(s) => {
            out.push_str("$dynamodb_SS::");
            list(s, out, |s, o| string(s, o));
        }
        Value::Ns(s) => {
            out.push_str("$dynamodb_NS::");
            list(s, out, decimal);
        }
        Value::Bs(s) => {
            out.push_str("$dynamodb_BS::");
            list(s, out, |b, o| blob(b, o));
        }
        Value::L(l) => list(l, out, write),
        Value::M(m) => {
            out.push('{');
            for (i, (k, v)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                field_name(k, out);
                out.push(':');
                write(v, out);
            }
            out.push('}');
        }
    }
}

/// An Ion value as read, before it is made a DynamoDB value.
#[derive(Debug, PartialEq)]
pub(crate) enum Ion {
    Null,
    Bool(bool),
    /// A number, as the text DynamoDB parses.
    Num(String),
    Str(String),
    Blob(Vec<u8>),
    List(Vec<Ion>, Option<String>),
    Struct(Vec<(String, Ion)>),
}

// fuzz:parser fuzz_import
/// Reads every top-level value of `text`, the version markers left out.
///
/// # Errors
/// Where the text is not Ion this reader takes.
pub(crate) fn read_all(text: &str) -> Result<Vec<Ion>, String> {
    let mut p = Parser {
        s: text.as_bytes(),
        at: 0,
    };
    let mut out = Vec::new();
    loop {
        p.space()?;
        if p.at >= p.s.len() {
            return Ok(out);
        }
        if p.rest().starts_with(MARKER.as_bytes()) {
            p.at += MARKER.len();
            continue;
        }
        out.push(p.value(0)?);
    }
}

struct Parser<'a> {
    s: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn rest(&self) -> &[u8] {
        self.s.get(self.at..).unwrap_or_default()
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.at).copied()
    }

    fn fail<T>(&self, why: &str) -> Result<T, String> {
        Err(format!("{why} at byte {}", self.at))
    }

    /// Skips white space and comments.
    fn space(&mut self) -> Result<(), String> {
        loop {
            match self.peek() {
                Some(c) if c.is_ascii_whitespace() => self.at += 1,
                Some(b'/') if self.rest().starts_with(b"//") => {
                    while self.peek().is_some_and(|c| c != b'\n') {
                        self.at += 1;
                    }
                }
                Some(b'/') if self.rest().starts_with(b"/*") => {
                    let Some(end) = find(self.rest(), b"*/") else {
                        return self.fail("an unterminated comment");
                    };
                    self.at += end + 2;
                }
                _ => return Ok(()),
            }
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Result<Ion, String> {
        if depth > MAX_DEPTH + 2 {
            return self.fail("nesting too deep");
        }
        self.space()?;
        let annotation = self.annotation()?;
        self.space()?;
        let v = match self.peek() {
            Some(b'{') if self.rest().starts_with(b"{{") => self.blob()?,
            Some(b'{') => self.structure(depth)?,
            Some(b'[') => Ion::List(self.items(depth, b']')?, None),
            Some(b'"') => Ion::Str(self.quoted(b'"')?),
            Some(b'\'') if self.rest().starts_with(b"'''") => Ion::Str(self.long_string()?),
            Some(b'\'') => Ion::Str(self.quoted(b'\'')?),
            Some(b'(') => return self.fail("an s-expression, which has no DynamoDB type"),
            Some(_) => self.atom()?,
            None => return self.fail("a missing value"),
        };
        Ok(match (v, annotation) {
            (Ion::List(l, _), Some(a)) => Ion::List(l, Some(a)),
            (v, _) => v,
        })
    }

    /// `name::` before a value, if there is one.
    fn annotation(&mut self) -> Result<Option<String>, String> {
        let start = self.at;
        let name = match self.peek() {
            Some(b'\'') if !self.rest().starts_with(b"'''") => Some(self.quoted(b'\'')?),
            Some(c) if c.is_ascii_alphabetic() || c == b'_' || c == b'$' => Some(self.word()),
            _ => None,
        };
        let Some(name) = name else { return Ok(None) };
        self.space()?;
        if self.rest().starts_with(b"::") {
            self.at += 2;
            return Ok(Some(name));
        }
        self.at = start;
        Ok(None)
    }

    fn word(&mut self) -> String {
        let start = self.at;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'$')
        {
            self.at += 1;
        }
        String::from_utf8_lossy(self.s.get(start..self.at).unwrap_or_default()).into_owned()
    }

    fn structure(&mut self, depth: usize) -> Result<Ion, String> {
        self.at += 1;
        let mut fields = Vec::new();
        loop {
            self.space()?;
            if self.eat(b'}') {
                return Ok(Ion::Struct(fields));
            }
            let name = match self.peek() {
                Some(b'"') => self.quoted(b'"')?,
                Some(b'\'') => self.quoted(b'\'')?,
                Some(c) if c.is_ascii_alphabetic() || c == b'_' || c == b'$' => self.word(),
                _ => return self.fail("a field name expected"),
            };
            self.space()?;
            if !self.eat(b':') {
                return self.fail("':' expected after a field name");
            }
            let v = self.value(depth + 1)?;
            fields.push((name, v));
            self.space()?;
            if !self.eat(b',') && self.peek() != Some(b'}') {
                return self.fail("',' or '}' expected");
            }
        }
    }

    fn items(&mut self, depth: usize, close: u8) -> Result<Vec<Ion>, String> {
        self.at += 1;
        let mut out = Vec::new();
        loop {
            self.space()?;
            if self.eat(close) {
                return Ok(out);
            }
            out.push(self.value(depth + 1)?);
            self.space()?;
            if !self.eat(b',') && self.peek() != Some(close) {
                return self.fail("',' or the end of the list expected");
            }
        }
    }

    fn blob(&mut self) -> Result<Ion, String> {
        self.at += 2;
        let Some(end) = find(self.rest(), b"}}") else {
            return self.fail("an unterminated blob");
        };
        let raw: String = self
            .rest()
            .get(..end)
            .unwrap_or_default()
            .iter()
            .filter(|c| !c.is_ascii_whitespace())
            .map(|c| char::from(*c))
            .collect();
        self.at += end + 2;
        if raw.starts_with('"') {
            return self.fail("a clob, which has no DynamoDB type");
        }
        aws_core::base64::decode(&raw)
            .map(Ion::Blob)
            .ok_or_else(|| format!("a blob that is not base64 at byte {}", self.at))
    }

    fn long_string(&mut self) -> Result<String, String> {
        let mut out = String::new();
        loop {
            self.at += 3;
            let Some(end) = find(self.rest(), b"'''") else {
                return self.fail("an unterminated long string");
            };
            let body = self.rest().get(..end).unwrap_or_default().to_vec();
            out.push_str(&unescape(&body).map_err(|e| format!("{e} at byte {}", self.at))?);
            self.at += end + 3;
            self.space()?;
            if !self.rest().starts_with(b"'''") {
                return Ok(out);
            }
        }
    }

    fn quoted(&mut self, quote: u8) -> Result<String, String> {
        self.at += 1;
        let start = self.at;
        loop {
            match self.peek() {
                None => return self.fail("an unterminated string"),
                Some(b'\\') => self.at += 2,
                Some(c) if c == quote => break,
                Some(_) => self.at += 1,
            }
        }
        let body = self.s.get(start..self.at).unwrap_or_default().to_vec();
        self.at += 1;
        unescape(&body).map_err(|e| format!("{e} at byte {start}"))
    }

    /// A keyword, typed null, or number.
    fn atom(&mut self) -> Result<Ion, String> {
        let start = self.at;
        while self.peek().is_some_and(|c| {
            !c.is_ascii_whitespace() && !b",]})".contains(&c) && !self.rest().starts_with(b"//")
        }) {
            self.at += 1;
        }
        let text = std::str::from_utf8(self.s.get(start..self.at).unwrap_or_default())
            .map_err(|_| format!("text that is not UTF-8 at byte {start}"))?;
        match text {
            "true" => return Ok(Ion::Bool(true)),
            "false" => return Ok(Ion::Bool(false)),
            t if t == "null" || t.starts_with("null.") => return Ok(Ion::Null),
            _ => {}
        }
        number(text)
            .map(Ion::Num)
            .ok_or_else(|| format!("'{text}' is not a value DynamoDB can hold, at byte {start}"))
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// An Ion integer, decimal or float as the text DynamoDB reads a number from: `103.` is `103`, `6d2` is `6E2`.
fn number(text: &str) -> Option<String> {
    let t = text.replace('_', "");
    let first = t.trim_start_matches(['-', '+']);
    if first.starts_with("0x") || first.starts_with("0b") || first.is_empty() {
        return None;
    }
    if !first.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        return None;
    }
    if t.contains('T') || t.matches('-').count() > 1 + usize::from(t.contains(['d', 'D', 'e', 'E']))
    {
        // A timestamp (`2020-01-01T`).
        return None;
    }
    let t = t.replace(['d', 'D'], "E");
    let t = match t.split_once(['e', 'E']) {
        Some((m, e)) => format!("{}E{e}", m.trim_end_matches('.')),
        None => t.trim_end_matches('.').to_owned(),
    };
    Number::parse(&t).ok().map(|_| t)
}

fn unescape(body: &[u8]) -> Result<String, String> {
    let text = std::str::from_utf8(body).map_err(|_| "text that is not UTF-8".to_owned())?;
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let e = chars.next().ok_or("an escape at the end of a string")?;
        let hex = |n: usize, chars: &mut std::str::Chars<'_>| -> Result<char, String> {
            let h: String = chars.by_ref().take(n).collect();
            u32::from_str_radix(&h, 16)
                .ok()
                .and_then(char::from_u32)
                .ok_or_else(|| format!("a bad escape \\{h}"))
        };
        out.push(match e {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            '0' => '\0',
            'a' => '\u{7}',
            'b' => '\u{8}',
            'f' => '\u{c}',
            'v' => '\u{b}',
            'x' => hex(2, &mut chars)?,
            'u' => hex(4, &mut chars)?,
            'U' => hex(8, &mut chars)?,
            '\n' => continue,
            other => other,
        });
    }
    Ok(out)
}

/// An Ion value as a DynamoDB one.
///
/// # Errors
/// What has no DynamoDB form (a set of mixed or wrong members, an empty set).
pub(crate) fn to_value(v: Ion) -> Result<Value, String> {
    Ok(match v {
        Ion::Null => Value::Null,
        Ion::Bool(b) => Value::Bool(b),
        Ion::Num(n) => Value::N(Number::parse(&n).map_err(|e| e.message(&n))?),
        Ion::Str(s) => Value::S(s),
        Ion::Blob(b) => Value::B(b),
        Ion::Struct(fields) => {
            let mut m = BTreeMap::new();
            for (k, v) in fields {
                m.insert(k, to_value(v)?);
            }
            Value::M(m)
        }
        Ion::List(items, None) => {
            Value::L(items.into_iter().map(to_value).collect::<Result<_, _>>()?)
        }
        Ion::List(items, Some(a)) => set(items, &a)?,
    })
}

fn set(items: Vec<Ion>, annotation: &str) -> Result<Value, String> {
    if items.is_empty() {
        return Err(format!("an empty {annotation} set"));
    }
    let wrong = || format!("a member of a {annotation} set of another type");
    Ok(match annotation {
        "$dynamodb_SS" => Value::Ss(
            items
                .into_iter()
                .map(|i| match i {
                    Ion::Str(s) => Ok(s),
                    _ => Err(wrong()),
                })
                .collect::<Result<BTreeSet<_>, _>>()?,
        ),
        "$dynamodb_NS" => Value::Ns(
            items
                .into_iter()
                .map(|i| match i {
                    Ion::Num(n) => Number::parse(&n).map_err(|e| e.message(&n)),
                    _ => Err(wrong()),
                })
                .collect::<Result<BTreeSet<_>, _>>()?,
        ),
        "$dynamodb_BS" => Value::Bs(
            items
                .into_iter()
                .map(|i| match i {
                    Ion::Blob(b) => Ok(b),
                    _ => Err(wrong()),
                })
                .collect::<Result<BTreeSet<_>, _>>()?,
        ),
        _ => Value::L(items.into_iter().map(to_value).collect::<Result<_, _>>()?),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use aws_expr::Item;

    use super::*;

    fn n(s: &str) -> Value {
        Value::N(Number::parse(s).unwrap())
    }

    #[test]
    fn what_an_export_writes_is_read_back() {
        let mut item = Item::new();
        item.insert("Id".into(), n("103"));
        item.insert("Price".into(), n("2000.5"));
        item.insert("Big".into(), n("1E+40"));
        item.insert(
            "Authors".into(),
            Value::Ss(["A".into(), "B\"q".into()].into()),
        );
        item.insert("Ns".into(), Value::Ns([Number::from_i64(1)].into()));
        item.insert("Bin".into(), Value::B(vec![0, 1, 2]));
        item.insert("Bs".into(), Value::Bs([vec![9]].into()));
        item.insert(
            "odd name".into(),
            Value::L(vec![Value::Null, Value::Bool(true)]),
        );
        item.insert(
            "M".into(),
            Value::M([("x\ny".into(), Value::S(String::new()))].into()),
        );
        let text = line("Item", &Value::M(item.clone()));
        assert!(text.starts_with("$ion_1_0 {Item:{"));
        let mut read = read_all(&text).unwrap();
        let Some(Ion::Struct(mut top)) = read.pop() else {
            panic!("{text}")
        };
        let Some((_, inner)) = top.pop() else {
            panic!()
        };
        assert_eq!(to_value(inner).unwrap(), Value::M(item));
    }

    #[test]
    fn the_guides_example_reads() {
        let text = "$ion_1_0 {\n Item:{\n Authors:$dynamodb_SS::[\"Author1\",\"Author2\"],\n Id:103.,\n InPublication:false,\n PageCount:6d2,\n Price:2d3,\n Title:'''Book ''' '''103''' // a comment\n }\n}\n";
        let read = read_all(text).unwrap();
        let [Ion::Struct(top)] = read.as_slice() else {
            panic!("{read:?}")
        };
        let Some((name, Ion::Struct(fields))) = top.first() else {
            panic!()
        };
        assert_eq!(name, "Item");
        let get = |k: &str| fields.iter().find(|(f, _)| f == k).map(|(_, v)| v).unwrap();
        assert_eq!(get("PageCount"), &Ion::Num("6E2".into()));
        assert_eq!(get("Id"), &Ion::Num("103".into()));
        assert_eq!(get("Title"), &Ion::Str("Book 103".into()));
        assert!(matches!(get("Authors"), Ion::List(_, Some(a)) if a == "$dynamodb_SS"));
    }

    #[test]
    fn what_has_no_dynamodb_form_is_refused() {
        for bad in [
            "{a:2020-01-01T}",
            "{a:(x y)}",
            "{a:{{\"clob\"}}}",
            "{a:",
            "{a:'x}",
            "{a 1}",
        ] {
            assert!(read_all(bad).is_err(), "{bad}");
        }
        assert!(
            to_value(Ion::List(
                vec![Ion::Num("1".into())],
                Some("$dynamodb_SS".into())
            ))
            .is_err()
        );
    }
}
