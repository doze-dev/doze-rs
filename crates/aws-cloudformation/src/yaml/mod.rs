//! A bounded YAML reader for templates: the subset CloudFormation reads (YAML 1.1 as SnakeYAML resolves it), into JSON.
//!
//! Block and flow collections, plain, quoted and block scalars, comments, anchors and aliases (expansion is counted against a
//! budget, so a "billion laughs" document is refused), and tags. A tag `!Ref`, `!GetAtt`, `!Condition` or `!<Name>` is the short
//! form of an intrinsic and becomes `{"Ref": ..}`, `{"Fn::GetAtt": [..]}`, `{"Condition": ..}` or `{"Fn::<Name>": ..}`. Not read:
//! complex keys (`? `), merge keys (`<<`), and more than one document. Depth and size are bounded, and every refusal carries
//! the line and column (1-based) the way AWS prints them: `YAML not well-formed. (line 1, column 6)`.
//!
//! Written here rather than taken from a crate: the reader is about 800 lines, a YAML crate is several times that in binary
//! size, and none reports positions in SnakeYAML's terms. Fuzzed by `fuzz_yaml`.

// fuzz:parser fuzz_yaml

mod block;
mod flow;
mod scalar;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

use serde_json::{Map, Value};

/// How deep collections may nest.
const MAX_DEPTH: usize = 100;
/// How many nodes a document may expand to, aliases included.
const MAX_NODES: usize = 2_000_000;

/// Where the reader stopped understanding the text, 1-based.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct YamlError {
    pub(crate) line: usize,
    pub(crate) column: usize,
}

type R<T> = Result<T, YamlError>;

/// Reads one YAML document into JSON.
///
/// # Errors
/// Where the text stops being YAML this reader understands.
pub(crate) fn parse(text: &str) -> R<Value> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let owned;
    let text = if text.contains('\r') {
        owned = text.replace("\r\n", "\n").replace('\r', "\n");
        owned.as_str()
    } else {
        text
    };
    let mut p = P {
        s: text,
        b: text.as_bytes(),
        i: 0,
        depth: 0,
        nodes: 0,
        anchors: HashMap::new(),
    };
    p.document()
}

/// The reader: the text, where it is, and what it has counted.
pub(super) struct P<'a> {
    pub(super) s: &'a str,
    pub(super) b: &'a [u8],
    pub(super) i: usize,
    depth: usize,
    nodes: usize,
    anchors: HashMap<String, Value>,
}

/// Whether `c` ends a token: a space, a tab, a line break or the end.
pub(super) fn blank(c: Option<u8>) -> bool {
    matches!(c, None | Some(b' ' | b'\t' | b'\n'))
}

impl P<'_> {
    pub(super) fn fail<T>(&self) -> R<T> {
        Err(self.error_at(self.i))
    }

    pub(super) fn error_at(&self, at: usize) -> YamlError {
        let at = at.min(self.s.len());
        let before = self.s.get(..at).unwrap_or_default();
        let line = before.matches('\n').count() + 1;
        let start = before.rfind('\n').map_or(0, |p| p + 1);
        let column = before.get(start..).map_or(0, |t| t.chars().count()) + 1;
        YamlError { line, column }
    }

    pub(super) fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    pub(super) fn peek_at(&self, k: usize) -> Option<u8> {
        self.b.get(self.i + k).copied()
    }

    /// The column of the reader, 0-based, in bytes (indentation is spaces).
    pub(super) fn col(&self) -> usize {
        let before = self.b.get(..self.i).unwrap_or_default();
        let start = before
            .iter()
            .rposition(|&c| c == b'\n')
            .map_or(0, |p| p + 1);
        self.i - start
    }

    pub(super) fn skip_space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.i += 1;
        }
    }

    /// Skips a comment if one starts here.
    pub(super) fn skip_comment(&mut self) {
        if self.peek() == Some(b'#') {
            while !matches!(self.peek(), None | Some(b'\n')) {
                self.i += 1;
            }
        }
    }

    pub(super) fn at_eol(&self) -> bool {
        matches!(self.peek(), None | Some(b'\n'))
    }

    /// After a node on one line: only spaces and a comment may follow.
    pub(super) fn end_of_line(&mut self) -> R<()> {
        self.skip_space();
        if self.peek() == Some(b'#')
            && matches!(self.b.get(self.i.wrapping_sub(1)), Some(b' ' | b'\t'))
        {
            self.skip_comment();
        }
        if self.at_eol() { Ok(()) } else { self.fail() }
    }

    /// Skips spaces, comments and line breaks. `true` when something else follows; the reader is on it. A tab in the
    /// indentation is refused.
    pub(super) fn skip_to_content(&mut self) -> R<bool> {
        loop {
            match self.peek() {
                None => return Ok(false),
                Some(b' ') => self.i += 1,
                Some(b'\t') => {
                    if self.indent_only_before() {
                        // A tab on a line with nothing else is only a blank line.
                        let save = self.i;
                        self.skip_space();
                        if self.at_eol() || self.peek() == Some(b'#') {
                            continue;
                        }
                        self.i = save;
                        return self.fail();
                    }
                    self.i += 1;
                }
                Some(b'\n') => self.i += 1,
                Some(b'#') => self.skip_comment(),
                Some(_) => return Ok(true),
            }
        }
    }

    fn indent_only_before(&self) -> bool {
        let before = self.b.get(..self.i).unwrap_or_default();
        before
            .iter()
            .rev()
            .take_while(|&&c| c != b'\n')
            .all(|&c| c == b' ')
    }

    /// Whether the reader is at a `---` or `...` line.
    pub(super) fn at_document_marker(&self) -> bool {
        self.col() == 0
            && (self.s.get(self.i..).is_some_and(|t| t.starts_with("---"))
                || self.s.get(self.i..).is_some_and(|t| t.starts_with("...")))
            && blank(self.peek_at(3))
    }

    fn document(&mut self) -> R<Value> {
        // Directives (`%YAML 1.1`) before the start marker.
        while self.skip_to_content()? && self.peek() == Some(b'%') && self.col() == 0 {
            while !self.at_eol() {
                self.i += 1;
            }
        }
        if self.skip_to_content()? && self.at_document_marker() && self.peek() == Some(b'-') {
            self.i += 3;
        }
        let value = if self.skip_to_content()? && !self.at_document_marker() {
            self.block_node(-1, true)?
        } else {
            Value::Null
        };
        if self.skip_to_content()? && self.at_document_marker() && self.peek() == Some(b'.') {
            self.i += 3;
        }
        if self.skip_to_content()? {
            return self.fail();
        }
        Ok(value)
    }

    pub(super) fn enter(&mut self) -> R<()> {
        self.depth += 1;
        self.count(1)?;
        if self.depth > MAX_DEPTH {
            return self.fail();
        }
        Ok(())
    }

    pub(super) fn leave(&mut self) {
        self.depth -= 1;
    }

    pub(super) fn count(&mut self, n: usize) -> R<()> {
        self.nodes += n;
        if self.nodes > MAX_NODES {
            return self.fail();
        }
        Ok(())
    }

    /// A node in block context, starting where the reader is (perhaps mid-line, after `- `), more indented than `parent`. `seq_ok`
    /// lets a block sequence sit at the parent's own indentation (a mapping value may be written that way).
    pub(super) fn block_node(&mut self, parent: isize, seq_ok: bool) -> R<Value> {
        self.enter()?;
        let (tag, anchor) = self.properties()?;
        let save = self.i;
        self.skip_space();
        let v = if (tag.is_some() || anchor.is_some())
            && (self.at_eol() || self.peek() == Some(b'#'))
        {
            self.skip_comment();
            self.node_below(parent, seq_ok)?
        } else {
            self.i = save;
            self.block_inner(parent)?
        };
        self.leave();
        self.finish(v, tag, anchor)
    }

    /// The node on the lines below, or null if nothing below is indented enough.
    pub(super) fn node_below(&mut self, parent: isize, seq_ok: bool) -> R<Value> {
        if !self.skip_to_content()? || self.at_document_marker() {
            return Ok(Value::Null);
        }
        let c = self.col() as isize;
        if c > parent {
            self.block_node(parent, seq_ok)
        } else if c == parent && seq_ok && self.peek() == Some(b'-') && blank(self.peek_at(1)) {
            self.block_seq(self.col())
        } else {
            Ok(Value::Null)
        }
    }

    fn block_inner(&mut self, parent: isize) -> R<Value> {
        let col = self.col();
        match self.peek() {
            Some(b'-') if blank(self.peek_at(1)) => self.block_seq(col),
            Some(b'[' | b'{') => {
                let v = self.flow_node()?;
                self.end_of_line()?;
                Ok(v)
            }
            Some(b'|' | b'>') => self.block_scalar(parent),
            Some(b'*') => {
                let v = self.alias()?;
                self.end_of_line()?;
                Ok(v)
            }
            Some(b'\'' | b'"') => {
                let start = self.i;
                let v = self.quoted()?;
                self.skip_space();
                if self.peek() == Some(b':') && blank(self.peek_at(1)) {
                    self.i = start;
                    return self.block_map(col);
                }
                self.end_of_line()?;
                Ok(Value::String(v))
            }
            Some(b'?') if blank(self.peek_at(1)) => self.fail(),
            _ => {
                if self.plain_key_ahead().is_some() {
                    return self.block_map(col);
                }
                self.plain_block(parent)
            }
        }
    }

    /// A tag and an anchor before a node, in either order.
    pub(super) fn properties(&mut self) -> R<(Option<String>, Option<String>)> {
        let (mut tag, mut anchor) = (None, None);
        loop {
            match self.peek() {
                Some(b'!') if tag.is_none() => {
                    let start = self.i;
                    while !blank(self.peek()) && !matches!(self.peek(), Some(b',' | b']' | b'}')) {
                        self.i += 1;
                    }
                    tag = Some(self.s.get(start..self.i).unwrap_or_default().to_owned());
                }
                Some(b'&') if anchor.is_none() => {
                    self.i += 1;
                    let name = self.anchor_name()?;
                    anchor = Some(name);
                }
                _ => return Ok((tag, anchor)),
            }
            self.skip_space();
        }
    }

    fn anchor_name(&mut self) -> R<String> {
        let start = self.i;
        while !blank(self.peek()) && !matches!(self.peek(), Some(b',' | b'[' | b']' | b'{' | b'}'))
        {
            self.i += 1;
        }
        if self.i == start {
            return self.fail();
        }
        Ok(self.s.get(start..self.i).unwrap_or_default().to_owned())
    }

    pub(super) fn alias(&mut self) -> R<Value> {
        let at = self.i;
        self.i += 1;
        let name = self.anchor_name()?;
        let Some(v) = self.anchors.get(&name).cloned() else {
            return Err(self.error_at(at));
        };
        self.count(size(&v))?;
        Ok(v)
    }

    /// Applies a node's tag and remembers its anchor.
    pub(super) fn finish(
        &mut self,
        v: Value,
        tag: Option<String>,
        anchor: Option<String>,
    ) -> R<Value> {
        let v = match tag {
            Some(t) => apply_tag(&t, v).ok_or_else(|| self.error_at(self.i))?,
            None => v,
        };
        if let Some(name) = anchor {
            self.anchors.insert(name, v.clone());
        }
        Ok(v)
    }
}

/// The number of nodes in a value (what an alias costs).
fn size(v: &Value) -> usize {
    match v {
        Value::Array(a) => 1 + a.iter().map(size).sum::<usize>(),
        Value::Object(o) => 1 + o.values().map(size).sum::<usize>(),
        _ => 1,
    }
}

/// A tagged node as CloudFormation reads it. `None` for a tag it refuses.
fn apply_tag(tag: &str, v: Value) -> Option<Value> {
    let as_text = |v: Value| match v {
        Value::Null => Value::String(String::new()),
        Value::Bool(b) => Value::String(b.to_string()),
        Value::Number(n) => Value::String(n.to_string()),
        other => other,
    };
    let one = |k: &str, v: Value| {
        let mut m = Map::new();
        m.insert(k.to_owned(), v);
        Value::Object(m)
    };
    match tag {
        "!!str" => Some(as_text(v)),
        "!!int" | "!!float" | "!!bool" | "!!null" | "!!map" | "!!seq" => Some(v),
        "!Ref" => Some(one("Ref", as_text(v))),
        "!Condition" => Some(one("Condition", as_text(v))),
        "!GetAtt" => Some(one(
            "Fn::GetAtt",
            match as_text(v) {
                Value::String(s) => match s.split_once('.') {
                    Some((a, b)) => Value::Array(vec![a.into(), b.into()]),
                    None => Value::String(s),
                },
                other => other,
            },
        )),
        t if t.starts_with("!!") => None,
        t => {
            let name = t.strip_prefix('!')?;
            if name.is_empty() {
                return Some(as_text(v));
            }
            Some(one(&format!("Fn::{name}"), as_text(v)))
        }
    }
}
