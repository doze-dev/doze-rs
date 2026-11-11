//! Strings: a token with no escape stays a slice of the body; one with an escape is decoded once into the arena's text.

use super::{D, R, Violation};
use crate::arena::Val;

impl D<'_> {
    /// A string token: `Str` if it had no escape, else decoded into the arena's text.
    pub(super) fn string(&mut self) -> R<Val> {
        self.expect(b'"')?;
        let start = self.pos;
        loop {
            match self.peek() {
                None => return Err(Violation::Syntax { at: self.pos }),
                Some(b'"') => {
                    let v = Val::Str(
                        u32::try_from(start).unwrap_or(0),
                        u32::try_from(self.pos - start).unwrap_or(0),
                    );
                    self.pos += 1;
                    return Ok(v);
                }
                Some(b'\\') => return self.escaped(start),
                Some(c) if c < 0x20 => return Err(Violation::Syntax { at: self.pos }),
                Some(_) => self.pos += 1,
            }
        }
    }

    pub(super) fn escaped(&mut self, start: usize) -> R<Val> {
        let off = self.ar.text.len();
        let r = self.escaped_into(start, off);
        if r.is_err() {
            self.ar.text.truncate(off);
        }
        r
    }

    // WHY: an escape means the decoded text exists nowhere in the body, so it is written once, straight into the arena's text.
    pub(super) fn escaped_into(&mut self, start: usize, off: usize) -> R<Val> {
        let src = self.src;
        // The decoded text is never longer than the token, so one reservation covers it.
        self.ar.text.reserve(self.string_end(self.pos) - start);
        self.ar
            .text
            .push_str(src.get(start..self.pos).unwrap_or(""));
        loop {
            match self.peek() {
                None => return Err(Violation::Syntax { at: self.pos }),
                Some(b'"') => {
                    self.pos += 1;
                    let len = self.ar.text.len() - off;
                    return Ok(Val::Esc(
                        u32::try_from(off).unwrap_or(0),
                        u32::try_from(len).unwrap_or(0),
                    ));
                }
                Some(b'\\') => {
                    self.pos += 1;
                    let c = self.peek().ok_or(Violation::Syntax { at: self.pos })?;
                    self.pos += 1;
                    let ch = match c {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => self.unicode()?,
                        _ => return Err(Violation::Syntax { at: self.pos }),
                    };
                    self.ar.text.push(ch);
                }
                Some(c) if c < 0x20 => return Err(Violation::Syntax { at: self.pos }),
                Some(_) => {
                    // A run with no quote, backslash or control byte is copied whole.
                    let run = self
                        .bytes()
                        .get(self.pos..)
                        .unwrap_or(&[])
                        .iter()
                        .take_while(|&&b| b != b'"' && b != b'\\' && b >= 0x20)
                        .count();
                    self.ar.text.push_str(
                        src.get(self.pos..self.pos + run)
                            .ok_or(Violation::Syntax { at: self.pos })?,
                    );
                    self.pos += run;
                }
            }
        }
    }

    /// Where the string that is open at `from` ends: the next quote that is not escaped.
    pub(super) fn string_end(&self, from: usize) -> usize {
        let mut i = from;
        while let Some(&c) = self.bytes().get(i) {
            match c {
                b'"' => return i,
                b'\\' => i += 2,
                _ => i += 1,
            }
        }
        self.src.len()
    }

    pub(super) fn hex4(&mut self) -> R<u32> {
        let s = self
            .src
            .get(self.pos..self.pos + 4)
            .ok_or(Violation::Syntax { at: self.pos })?;
        self.pos += 4;
        u32::from_str_radix(s, 16).map_err(|_| Violation::Syntax { at: self.pos })
    }

    pub(super) fn unicode(&mut self) -> R<char> {
        let hi = self.hex4()?;
        let cp = if (0xD800..0xDC00).contains(&hi) {
            self.lit("\\u")?;
            let lo = self.hex4()?;
            if !(0xDC00..0xE000).contains(&lo) {
                return Err(Violation::Syntax { at: self.pos });
            }
            0x1_0000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
        } else {
            hi
        };
        char::from_u32(cp).ok_or(Violation::Syntax { at: self.pos })
    }

    pub(super) fn text(&self, v: Val) -> &str {
        self.ar.str_of(self.src, v).unwrap_or("")
    }
}
