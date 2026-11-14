//! Flow collections (`[a, b]`, `{k: v}`) and the scalars inside them.

use serde_json::{Map, Value};

use super::scalar::resolve;
use super::{P, R};

impl P<'_> {
    /// A flow collection or scalar (inside `[ ]` or `{ }`, or one starting with them).
    pub(super) fn flow_node(&mut self) -> R<Value> {
        self.enter()?;
        let (tag, anchor) = self.properties()?;
        self.skip_flow_space()?;
        let v = match self.peek() {
            Some(b'[') => self.flow_seq()?,
            Some(b'{') => self.flow_map()?,
            Some(b'\'' | b'"') => Value::String(self.quoted()?),
            Some(b'*') => self.alias()?,
            Some(b',' | b']' | b'}') | None => Value::Null,
            Some(b'#' | b'|' | b'>' | b'@' | b'`' | b'%') => return self.fail(),
            _ => self.plain_flow()?,
        };
        self.leave();
        self.finish(v, tag, anchor)
    }

    /// A plain scalar in flow context, which may run over several lines.
    fn plain_flow(&mut self) -> R<Value> {
        let mut text = self.plain_line(true)?;
        loop {
            let save = self.i;
            let mut breaks = 0;
            while self.peek() == Some(b'\n') {
                self.i += 1;
                breaks += 1;
                self.skip_space();
            }
            if breaks == 0
                || matches!(
                    self.peek(),
                    None | Some(b'\n' | b'#' | b',' | b']' | b'}' | b':')
                )
            {
                self.i = save;
                break;
            }
            let more = self.plain_line(true)?;
            if breaks == 1 {
                text.push(' ');
            } else {
                text.extend(std::iter::repeat_n('\n', breaks - 1));
            }
            text.push_str(&more);
        }
        Ok(resolve(&text))
    }

    /// Spaces, line breaks and comments inside a flow collection.
    fn skip_flow_space(&mut self) -> R<()> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\n') => self.i += 1,
                Some(b'#')
                    if matches!(
                        self.b.get(self.i.wrapping_sub(1)),
                        Some(b' ' | b'\t' | b'\n') | None
                    ) =>
                {
                    self.skip_comment();
                }
                _ => return Ok(()),
            }
        }
    }

    fn flow_seq(&mut self) -> R<Value> {
        self.i += 1;
        let mut items = Vec::new();
        loop {
            self.skip_flow_space()?;
            if self.peek() == Some(b']') {
                self.i += 1;
                return Ok(Value::Array(items));
            }
            let item = self.flow_node()?;
            self.skip_flow_space()?;
            // A single pair inside a sequence: `[a: b]`.
            let item = if self.peek() == Some(b':') {
                self.i += 1;
                self.skip_flow_space()?;
                let value = self.flow_node()?;
                let mut m = Map::new();
                m.insert(key_text(item), value);
                self.skip_flow_space()?;
                Value::Object(m)
            } else {
                item
            };
            items.push(item);
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {}
                _ => return self.fail(),
            }
        }
    }

    fn flow_map(&mut self) -> R<Value> {
        self.i += 1;
        let mut map = Map::new();
        loop {
            self.skip_flow_space()?;
            if self.peek() == Some(b'}') {
                self.i += 1;
                return Ok(Value::Object(map));
            }
            let key_at = self.i;
            let key = key_text(self.flow_node()?);
            self.skip_flow_space()?;
            let value = if self.peek() == Some(b':') {
                self.i += 1;
                self.skip_flow_space()?;
                if matches!(self.peek(), Some(b',' | b'}')) {
                    Value::Null
                } else {
                    self.flow_node()?
                }
            } else {
                Value::Null
            };
            if map.insert(key, value).is_some() {
                return Err(self.error_at(key_at));
            }
            self.skip_flow_space()?;
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {}
                _ => return self.fail(),
            }
        }
    }
}

/// A key as text, whatever it resolved to.
fn key_text(v: Value) -> String {
    match v {
        Value::String(s) => s,
        Value::Null => String::new(),
        other => other.to_string(),
    }
}
