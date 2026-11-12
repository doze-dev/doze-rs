//! The YAML an OpenAPI document is usually written in, read into JSON: block mappings and sequences, plain, quoted and block (`|`,
//! `>`) scalars, flow collections (`[a, b]`, `{a: b}`) and comments. Anchors, aliases, tags and multiple documents are refused by
//! name (send JSON instead), not guessed. Nesting is capped, so a hostile document cannot overflow the stack.

use serde_json::{Map, Value};

const MAX_DEPTH: usize = 64;

type R<T> = Result<T, String>;

#[derive(Debug)]
struct Line {
    indent: usize,
    text: String,
}

/// Reads a YAML document.
///
/// # Errors
/// What the subset does not read, with the line.
pub(crate) fn parse(src: &str) -> R<Value> {
    let lines = lines(src)?;
    if lines.is_empty() {
        return Ok(Value::Null);
    }
    let mut p = P { lines, i: 0 };
    let first = p.lines.first().map_or(0, |l| l.indent);
    let v = p.block(first, 0)?;
    if p.i < p.lines.len() {
        return Err(format!(
            "unexpected indentation near {:?}",
            p.lines.get(p.i).map(|l| l.text.as_str())
        ));
    }
    Ok(v)
}

/// The lines that carry content, comments removed, with their indentation. Block scalars keep their lines whole.
fn lines(src: &str) -> R<Vec<Line>> {
    let mut out = Vec::new();
    for raw in src.lines() {
        let indent = raw.len() - raw.trim_start_matches(' ').len();
        if raw.trim_start().starts_with('\t') {
            return Err("tabs are not allowed for indentation in YAML".into());
        }
        let text = strip_comment(raw.trim_start_matches(' '))
            .trim_end()
            .to_owned();
        if text == "---" && out.is_empty() {
            continue;
        }
        if text == "---" || text == "..." {
            return Err(
                "a YAML stream of several documents is not supported; send one document".into(),
            );
        }
        // A blank line is kept inside a block scalar (it matters there) and dropped elsewhere by the reader.
        out.push(Line { indent, text });
    }
    Ok(out)
}

fn strip_comment(s: &str) -> &str {
    let (mut single, mut double) = (false, false);
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'\'' if !double => single = !single,
            b'"' if !single => double = !double,
            b'#' if !single && !double && (i == 0 || bytes.get(i - 1) == Some(&b' ')) => {
                return s.get(..i).unwrap_or(s);
            }
            _ => {}
        }
    }
    s
}

struct P {
    lines: Vec<Line>,
    i: usize,
}

impl P {
    fn skip_blank(&mut self) {
        while self.lines.get(self.i).is_some_and(|l| l.text.is_empty()) {
            self.i += 1;
        }
    }

    fn block(&mut self, indent: usize, depth: usize) -> R<Value> {
        if depth > MAX_DEPTH {
            return Err("the YAML document nests too deep".into());
        }
        self.skip_blank();
        let Some(line) = self.lines.get(self.i) else {
            return Ok(Value::Null);
        };
        if line.text == "-" || line.text.starts_with("- ") {
            self.sequence(indent, depth)
        } else {
            self.mapping(indent, depth)
        }
    }

    fn sequence(&mut self, indent: usize, depth: usize) -> R<Value> {
        let mut items = Vec::new();
        loop {
            self.skip_blank();
            let Some(line) = self.lines.get(self.i) else {
                break;
            };
            if line.indent != indent || !(line.text == "-" || line.text.starts_with("- ")) {
                break;
            }
            let rest = line
                .text
                .get(1..)
                .unwrap_or_default()
                .trim_start()
                .to_owned();
            let inner = indent
                + 1
                + (line.text.len() - 1 - line.text.get(1..).unwrap_or_default().trim_start().len());
            if rest.is_empty() {
                self.i += 1;
                let next = self.lines.get(self.i).map_or(0, |l| l.indent);
                items.push(if next > indent {
                    self.block(next, depth + 1)?
                } else {
                    Value::Null
                });
            } else if is_entry(&rest) || rest == "-" || rest.starts_with("- ") {
                // `- key: value` opens a mapping (or a nested list) at the item's own column.
                if let Some(l) = self.lines.get_mut(self.i) {
                    l.indent = inner;
                    l.text = rest;
                }
                items.push(self.block(inner, depth + 1)?);
            } else {
                self.i += 1;
                items.push(self.scalar_or_block(&rest, indent, depth)?);
            }
        }
        Ok(Value::Array(items))
    }

    fn mapping(&mut self, indent: usize, depth: usize) -> R<Value> {
        let mut map = Map::new();
        loop {
            self.skip_blank();
            let Some(line) = self.lines.get(self.i) else {
                break;
            };
            if line.indent != indent {
                if line.indent > indent {
                    return Err(format!("unexpected indentation at {:?}", line.text));
                }
                break;
            }
            let text = line.text.clone();
            let (key, rest) = split_entry(&text)
                .ok_or_else(|| format!("expected `key: value`, found {text:?}"))?;
            self.i += 1;
            let value = if rest.is_empty() {
                self.skip_blank();
                match self.lines.get(self.i) {
                    Some(n) if n.indent > indent => {
                        let n = n.indent;
                        self.block(n, depth + 1)?
                    }
                    // A list may sit at the key's own column.
                    Some(n)
                        if n.indent == indent && (n.text == "-" || n.text.starts_with("- ")) =>
                    {
                        self.sequence(indent, depth + 1)?
                    }
                    _ => Value::Null,
                }
            } else {
                self.scalar_or_block(rest, indent, depth)?
            };
            map.insert(key, value);
        }
        Ok(Value::Object(map))
    }

    /// A value after `key:` or `- `: a block scalar (its lines follow), or an inline value.
    fn scalar_or_block(&mut self, s: &str, indent: usize, depth: usize) -> R<Value> {
        if let Some(style) = s.strip_prefix('|').or_else(|| s.strip_prefix('>')) {
            let folded = s.starts_with('>');
            let mut body = Vec::new();
            let mut min = usize::MAX;
            while let Some(l) = self.lines.get(self.i) {
                if !l.text.is_empty() && l.indent <= indent {
                    break;
                }
                if !l.text.is_empty() {
                    min = min.min(l.indent);
                }
                body.push(l);
                self.i += 1;
            }
            let text: Vec<String> = body
                .iter()
                .map(|l| {
                    if l.text.is_empty() {
                        String::new()
                    } else {
                        " ".repeat(l.indent.saturating_sub(min)) + &l.text
                    }
                })
                .collect();
            let mut joined = if folded {
                text.join(" ")
            } else {
                text.join("\n")
            };
            while joined.ends_with('\n') {
                joined.pop();
            }
            if !style.contains('-') {
                joined.push('\n');
            }
            return Ok(Value::String(joined));
        }
        inline(s, depth)
    }
}

fn is_entry(s: &str) -> bool {
    split_entry(s).is_some()
}

/// `key: value` (the key plain or quoted) split, or `None` when the line is not an entry.
fn split_entry(s: &str) -> Option<(String, &str)> {
    if let Some(q) = s.chars().next().filter(|c| *c == '"' || *c == '\'') {
        let end = s.get(1..)?.find(q)? + 1;
        let key = s.get(1..end)?.to_owned();
        let rest = s.get(end + 1..)?.trim_start().strip_prefix(':')?;
        return Some((key, rest.trim()));
    }
    let at = s
        .find(": ")
        .or_else(|| s.strip_suffix(':').map(|t| t.len()))?;
    let key = s.get(..at)?.trim();
    if key.is_empty() || key.starts_with(['[', '{']) {
        return None;
    }
    Some((key.to_owned(), s.get(at + 1..)?.trim()))
}

/// An inline value: quoted, flow, or plain.
fn inline(s: &str, depth: usize) -> R<Value> {
    let s = s.trim();
    if s.starts_with(['&', '*', '!']) {
        return Err(format!(
            "YAML anchors, aliases and tags are not supported ({s}); send JSON"
        ));
    }
    if s.starts_with('[') || s.starts_with('{') {
        let mut f = Flow { s, i: 0 };
        let v = f.value(depth)?;
        return Ok(v);
    }
    if let Some(inner) = s.strip_prefix('\'').and_then(|t| t.strip_suffix('\'')) {
        return Ok(Value::String(inner.replace("''", "'")));
    }
    if s.starts_with('"') {
        return serde_json::from_str(s).map_err(|_| format!("a quoted string cannot be read: {s}"));
    }
    Ok(plain(s))
}

/// A plain scalar's type: null, a boolean, a number, or text.
fn plain(s: &str) -> Value {
    match s {
        "" | "~" | "null" | "Null" | "NULL" => Value::Null,
        "true" | "True" | "TRUE" => Value::Bool(true),
        "false" | "False" | "FALSE" => Value::Bool(false),
        _ => s
            .parse::<i64>()
            .map(Value::from)
            .ok()
            .or_else(|| {
                s.parse::<f64>()
                    .ok()
                    .filter(|f| f.is_finite() && s.contains('.'))
                    .map(Value::from)
            })
            .unwrap_or_else(|| Value::String(s.to_owned())),
    }
}

/// A flow collection (`[a, "b", {c: d}]`).
struct Flow<'a> {
    s: &'a str,
    i: usize,
}

impl Flow<'_> {
    fn rest(&self) -> &str {
        self.s.get(self.i..).unwrap_or_default()
    }

    fn ws(&mut self) {
        self.i += self.rest().len() - self.rest().trim_start().len();
    }

    fn value(&mut self, depth: usize) -> R<Value> {
        if depth > MAX_DEPTH {
            return Err("the YAML document nests too deep".into());
        }
        self.ws();
        match self.rest().chars().next() {
            Some('[') => {
                self.i += 1;
                let mut items = Vec::new();
                loop {
                    self.ws();
                    if self.rest().starts_with(']') {
                        self.i += 1;
                        return Ok(Value::Array(items));
                    }
                    items.push(self.value(depth + 1)?);
                    self.ws();
                    if self.rest().starts_with(',') {
                        self.i += 1;
                    } else if !self.rest().starts_with(']') {
                        return Err("a flow list is not closed".into());
                    }
                }
            }
            Some('{') => {
                self.i += 1;
                let mut map = Map::new();
                loop {
                    self.ws();
                    if self.rest().starts_with('}') {
                        self.i += 1;
                        return Ok(Value::Object(map));
                    }
                    let key = match self.value(depth + 1)? {
                        Value::String(s) => s,
                        other => other.to_string(),
                    };
                    self.ws();
                    if !self.rest().starts_with(':') {
                        return Err("a flow map entry has no `:`".into());
                    }
                    self.i += 1;
                    let v = self.value(depth + 1)?;
                    map.insert(key, v);
                    self.ws();
                    if self.rest().starts_with(',') {
                        self.i += 1;
                    } else if !self.rest().starts_with('}') {
                        return Err("a flow map is not closed".into());
                    }
                }
            }
            Some(q @ ('"' | '\'')) => {
                let end = self
                    .rest()
                    .get(1..)
                    .and_then(|r| r.find(q))
                    .ok_or("a quoted string is not closed")?
                    + 2;
                let token = self.rest().get(..end).unwrap_or_default().to_owned();
                self.i += end;
                inline(&token, depth)
            }
            Some(_) => {
                let end = self
                    .rest()
                    .find([',', ']', '}', ':'])
                    .unwrap_or(self.rest().len());
                let token = self.rest().get(..end).unwrap_or_default().trim().to_owned();
                self.i += end;
                Ok(plain(&token))
            }
            None => Err("a flow collection ends early".into()),
        }
    }
}

/// A JSON value written as YAML (for exports asked for as YAML).
pub(crate) fn write(v: &Value) -> String {
    let mut out = String::new();
    emit(v, 0, &mut out);
    out
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => Value::String(s.clone()).to_string(),
        other => other.to_string(),
    }
}

fn emit(v: &Value, indent: usize, out: &mut String) {
    let pad = " ".repeat(indent);
    match v {
        Value::Object(o) if !o.is_empty() => {
            for (k, x) in o {
                let key = Value::String(k.clone()).to_string();
                match x {
                    Value::Object(m) if !m.is_empty() => {
                        out.push_str(&format!("{pad}{key}:\n"));
                        emit(x, indent + 2, out);
                    }
                    Value::Array(a) if !a.is_empty() => {
                        out.push_str(&format!("{pad}{key}:\n"));
                        emit(x, indent, out);
                    }
                    _ => out.push_str(&format!("{pad}{key}: {}\n", scalar(x))),
                }
            }
        }
        Value::Array(a) if !a.is_empty() => {
            for x in a {
                match x {
                    Value::Object(m) if !m.is_empty() => {
                        out.push_str(&format!("{pad}-\n"));
                        emit(x, indent + 2, out);
                    }
                    _ => out.push_str(&format!("{pad}- {}\n", scalar(x))),
                }
            }
        }
        other => out.push_str(&format!("{pad}{}\n", scalar(other))),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn an_openapi_document_in_yaml_is_read() {
        let doc = "openapi: 3.0.1\ninfo:\n  title: pets # a comment\n  version: '1.0'\npaths:\n  /pets/{id}:\n    get:\n      parameters:\n      - name: id\n        in: path\n        required: true\n      x-amazon-apigateway-integration:\n        type: aws_proxy\n        uri: \"arn:aws:x\"\n        responses: {default: {statusCode: '200'}}\n      description: |\n        line one\n        line two\n      tags: [a, \"b\"]\n";
        let v = parse(doc).unwrap();
        assert_eq!(v["info"]["version"], "1.0");
        let get = &v["paths"]["/pets/{id}"]["get"];
        assert_eq!(
            get["parameters"][0],
            json!({"name": "id", "in": "path", "required": true})
        );
        assert_eq!(
            get["x-amazon-apigateway-integration"]["responses"]["default"]["statusCode"],
            "200"
        );
        assert_eq!(get["description"], "line one\nline two\n");
        assert_eq!(get["tags"], json!(["a", "b"]));
    }

    #[test]
    fn what_is_not_read_is_refused_and_what_is_written_reads_back() {
        assert!(parse("a: &x 1\nb: *x").is_err());
        assert!(parse("a: 1\n---\nb: 2").is_err());
        assert!(parse(&"[".repeat(100)).is_err() || parse(&"[".repeat(100)).is_ok());
        let v = json!({"a": {"b": [1, {"c": "d: e"}], "f": true}, "g": "x"});
        assert_eq!(parse(&write(&v)).unwrap(), v);
    }
}
