//! Block collections: sequences of `- ` entries and mappings of `key: value` lines, by indentation.

use serde_json::{Map, Value};

use super::{P, R, blank};

impl P<'_> {
    pub(super) fn block_seq(&mut self, indent: usize) -> R<Value> {
        let mut items = Vec::new();
        loop {
            self.i += 1;
            self.skip_space();
            let item = if self.at_eol() || self.peek() == Some(b'#') {
                self.skip_comment();
                self.node_below(indent as isize, false)?
            } else {
                self.block_node(indent as isize, false)?
            };
            items.push(item);
            if !self.skip_to_content()? || self.at_document_marker() {
                break;
            }
            let c = self.col();
            if c < indent || (c == indent && !(self.peek() == Some(b'-') && blank(self.peek_at(1))))
            {
                break;
            }
            if c > indent {
                return self.fail();
            }
        }
        Ok(Value::Array(items))
    }

    pub(super) fn block_map(&mut self, indent: usize) -> R<Value> {
        let mut map = Map::new();
        loop {
            let key_at = self.i;
            let key = self.block_key()?;
            self.skip_space();
            if !(self.peek() == Some(b':') && blank(self.peek_at(1))) {
                return self.fail();
            }
            self.i += 1;
            self.skip_space();
            let value = if self.at_eol() || self.peek() == Some(b'#') {
                self.skip_comment();
                self.node_below(indent as isize, true)?
            } else {
                self.inline_value(indent)?
            };
            if map.insert(key, value).is_some() {
                // (memory) AWS refuses a template with a key given twice in one mapping.
                return Err(self.error_at(key_at));
            }
            if !self.skip_to_content()? || self.at_document_marker() {
                break;
            }
            let c = self.col();
            if c < indent {
                break;
            }
            if c > indent || (self.peek() == Some(b'-') && blank(self.peek_at(1))) {
                return self.fail();
            }
        }
        Ok(Value::Object(map))
    }

    /// A mapping key on one line: quoted, or plain up to the `: `.
    fn block_key(&mut self) -> R<String> {
        match self.peek() {
            Some(b'\'' | b'"') => self.quoted(),
            Some(b'[' | b'{' | b'?' | b'*' | b'&' | b'!' | b'|' | b'>') => self.fail(),
            _ => {
                let Some(end) = self.plain_key_ahead() else {
                    return self.fail();
                };
                let key = self
                    .s
                    .get(self.i..end)
                    .unwrap_or_default()
                    .trim_end()
                    .to_owned();
                self.i = end;
                Ok(key)
            }
        }
    }

    /// A value after `key: ` on the same line.
    pub(super) fn inline_value(&mut self, indent: usize) -> R<Value> {
        self.enter()?;
        let (tag, anchor) = self.properties()?;
        self.skip_space();
        let v = if (tag.is_some() || anchor.is_some())
            && (self.at_eol() || self.peek() == Some(b'#'))
        {
            self.skip_comment();
            self.node_below(indent as isize, true)?
        } else {
            match self.peek() {
                Some(b'[' | b'{') => {
                    let v = self.flow_node()?;
                    self.end_of_line()?;
                    v
                }
                Some(b'|' | b'>') => self.block_scalar(indent as isize)?,
                Some(b'*') => {
                    let v = self.alias()?;
                    self.end_of_line()?;
                    v
                }
                Some(b'\'' | b'"') => {
                    let v = Value::String(self.quoted()?);
                    self.end_of_line()?;
                    v
                }
                Some(b'-') if blank(self.peek_at(1)) => return self.fail(),
                _ => self.plain_block(indent as isize)?,
            }
        };
        self.leave();
        self.finish(v, tag, anchor)
    }
}
