//! A JSON reader that remembers where each value began, so a pattern that is wrong can be refused with the line and column AWS prints.
//! It reads only patterns; events and everything else go through `serde_json`.
//!
//! The words of a syntax failure follow Jackson, the parser EventBridge uses. Only the unquoted-key sentence is recorded (2026-11-09);
//! the others are Jackson's usual ones from memory.

/// Nesting deeper than this is refused, so a hostile pattern cannot overflow the stack.
const MAX_DEPTH: usize = 64;

/// A value and the byte at which it began.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Spanned {
    pub(crate) at: usize,
    pub(crate) v: Json,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Spanned>),
    Obj(Vec<(String, Spanned)>),
}

/// Why the text is not JSON: Jackson's sentence, and the byte it points at.
#[derive(Debug)]
pub(crate) struct Syntax {
    pub(crate) message: String,
    pub(crate) at: usize,
}

struct Reader<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

/// Reads `text` as one JSON value.
///
/// # Errors
/// The first thing wrong with it.
pub(crate) fn read(text: &str) -> Result<Spanned, Syntax> {
    let mut r = Reader {
        text,
        bytes: text.as_bytes(),
        pos: 0,
    };
    let v = r.value(0)?;
    r.blank();
    match r.peek() {
        None => Ok(v),
        Some(_) => Err(r.unexpected("expected end-of-input")),
    }
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn blank(&mut self) {
        while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
            self.pos += 1;
        }
    }

    fn unexpected(&self, expected: &str) -> Syntax {
        match self.text.get(self.pos..).and_then(|s| s.chars().next()) {
            Some(c) => Syntax {
                message: format!(
                    "Unexpected character ('{c}' (code {})): {expected}",
                    u32::from(c)
                ),
                at: self.pos,
            },
            None => Syntax {
                message: "Unexpected end-of-input".to_owned(),
                at: self.pos,
            },
        }
    }

    fn value(&mut self, depth: usize) -> Result<Spanned, Syntax> {
        if depth > MAX_DEPTH {
            return Err(Syntax {
                message: "Document nesting depth exceeds the maximum allowed".to_owned(),
                at: self.pos,
            });
        }
        self.blank();
        let at = self.pos;
        let v = match self.peek() {
            Some(b'{') => self.object(depth)?,
            Some(b'[') => self.array(depth)?,
            Some(b'"') => Json::Str(self.string()?),
            Some(b't') => self.word("true", Json::Bool(true))?,
            Some(b'f') => self.word("false", Json::Bool(false))?,
            Some(b'n') => self.word("null", Json::Null)?,
            Some(b'-' | b'0'..=b'9') => self.number()?,
            _ => {
                return Err(self.unexpected(
                    "expected a valid value (JSON String, Number, Array, Object or token 'null', 'true' or 'false')",
                ));
            }
        };
        Ok(Spanned { at, v })
    }

    fn word(&mut self, word: &str, v: Json) -> Result<Json, Syntax> {
        if self
            .text
            .get(self.pos..)
            .is_some_and(|s| s.starts_with(word))
        {
            self.pos += word.len();
            Ok(v)
        } else {
            Err(self.unexpected(&format!(
                "expected 'true', 'false' or 'null' (a token starting '{word}' was not completed)"
            )))
        }
    }

    fn number(&mut self) -> Result<Json, Syntax> {
        let start = self.pos;
        while self
            .peek()
            .is_some_and(|b| matches!(b, b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'))
        {
            self.pos += 1;
        }
        self.text
            .get(start..self.pos)
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|n| n.is_finite())
            .map(Json::Num)
            .ok_or(Syntax {
                message: "Invalid numeric value".to_owned(),
                at: start,
            })
    }

    fn string(&mut self) -> Result<String, Syntax> {
        let open = self.pos;
        self.pos += 1;
        let mut out = String::new();
        loop {
            let rest = self.text.get(self.pos..).unwrap_or_default();
            let Some(c) = rest.chars().next() else {
                return Err(Syntax {
                    message: "Unexpected end-of-input in VALUE_STRING".to_owned(),
                    at: open,
                });
            };
            self.pos += c.len_utf8();
            match c {
                '"' => return Ok(out),
                '\\' => out.push(self.escape()?),
                c if u32::from(c) < 0x20 => {
                    self.pos -= 1;
                    return Err(self.unexpected(
                        "has to be escaped using backslash to be included in string value",
                    ));
                }
                c => out.push(c),
            }
        }
    }

    fn escape(&mut self) -> Result<char, Syntax> {
        let c = self.peek().ok_or_else(|| self.unexpected(""))?;
        self.pos += 1;
        Ok(match c {
            b'n' => '\n',
            b't' => '\t',
            b'r' => '\r',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'"' | b'\\' | b'/' => char::from(c),
            b'u' => self.unicode()?,
            _ => {
                self.pos -= 1;
                return Err(
                    self.unexpected("(code is not a valid escape) was not expecting this escape")
                );
            }
        })
    }

    fn unicode(&mut self) -> Result<char, Syntax> {
        let hex = self.text.get(self.pos..self.pos + 4).unwrap_or_default();
        let n = u32::from_str_radix(hex, 16)
            .map_err(|_| self.unexpected("expected a hex digit for a unicode escape"))?;
        self.pos += 4;
        // A lone surrogate half is not a character; it reads as the replacement character.
        Ok(char::from_u32(n).unwrap_or('\u{fffd}'))
    }

    fn array(&mut self, depth: usize) -> Result<Json, Syntax> {
        self.pos += 1;
        let mut items = Vec::new();
        self.blank();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.blank();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err(self.unexpected("was expecting comma to separate Array entries")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, Syntax> {
        self.pos += 1;
        let mut fields = Vec::new();
        self.blank();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Obj(fields));
        }
        loop {
            self.blank();
            if self.peek() != Some(b'"') {
                return Err(self.unexpected("was expecting double-quote to start field name"));
            }
            let key = self.string()?;
            self.blank();
            if self.peek() != Some(b':') {
                return Err(
                    self.unexpected("was expecting a colon to separate field name and value")
                );
            }
            self.pos += 1;
            fields.push((key, self.value(depth + 1)?));
            self.blank();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Obj(fields));
                }
                _ => return Err(self.unexpected("was expecting comma to separate Object entries")),
            }
        }
    }
}

/// The 1-based line and column of byte `at` in `text`.
pub(crate) fn line_and_column(text: &str, at: usize) -> (usize, usize) {
    let before = text.get(..at.min(text.len())).unwrap_or_default();
    let line = before.bytes().filter(|b| *b == b'\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, column)
}
