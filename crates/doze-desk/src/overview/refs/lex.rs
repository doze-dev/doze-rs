//! Tokens of an HCL file, enough to find its blocks, a few of their attributes and the references in them: words
//! (identifiers and numbers), string literals, punctuation and line ends. Comments are dropped. A string's
//! interpolations (`${…}`, `%{…}`) and a heredoc's are lexed as code, so a reference inside one is seen, as HCL's own
//! parser sees it; the text around them is not code, so `"app.web"` in quotes is not a reference.

/// What a token is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Tok<'s> {
    /// An identifier or a number: `app`, `migrate-orders`, `8080`.
    Word(&'s str),
    /// A string literal or a heredoc, as written between its quotes (escapes undone).
    Str(String),
    Punct(char),
    Newline,
}

/// A token and where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Token<'s> {
    pub(crate) tok: Tok<'s>,
    /// The line it starts on, from 1.
    pub(crate) line: usize,
    /// Its bytes in the source.
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// How deep interpolations may nest before the rest is skipped: a file is not a stack overflow.
const MAX_NEST: usize = 32;

struct Lexer<'s> {
    src: &'s str,
    pos: usize,
    line: usize,
    out: Vec<Token<'s>>,
}

fn word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b >= 0x80
}

/// The tokens of `src`.
pub(crate) fn lex(src: &str) -> Vec<Token<'_>> {
    let mut l = Lexer {
        src,
        pos: 0,
        line: 1,
        out: Vec::new(),
    };
    l.code(false, 0);
    l.out
}

impl<'s> Lexer<'s> {
    fn peek(&self, ahead: usize) -> Option<u8> {
        self.src.as_bytes().get(self.pos + ahead).copied()
    }

    fn push(&mut self, tok: Tok<'s>, start: usize, line: usize) {
        self.out.push(Token {
            tok,
            line,
            start,
            end: self.pos,
        });
    }

    /// Code, until the end, or (`interp`) the `}` that closes an interpolation.
    fn code(&mut self, interp: bool, nest: usize) {
        let mut depth = 0usize;
        while let Some(b) = self.peek(0) {
            let (start, line) = (self.pos, self.line);
            match b {
                b'\n' => {
                    self.pos += 1;
                    self.push(Tok::Newline, start, line);
                    self.line += 1;
                }
                b'#' => self.skip_line(),
                b'/' if self.peek(1) == Some(b'/') => self.skip_line(),
                b'/' if self.peek(1) == Some(b'*') => self.block_comment(),
                b'"' => self.string(nest),
                b'<' if self.peek(1) == Some(b'<') && self.heredoc(nest) => {}
                b'}' if interp && depth == 0 => {
                    self.pos += 1;
                    return;
                }
                _ if word_byte(b) => {
                    while self.peek(0).is_some_and(word_byte) {
                        self.pos += 1;
                    }
                    let w = self.src.get(start..self.pos).unwrap_or("");
                    self.push(Tok::Word(w), start, line);
                }
                _ if b.is_ascii_whitespace() => self.pos += 1,
                _ => {
                    match b {
                        b'{' => depth += 1,
                        b'}' => depth = depth.saturating_sub(1),
                        _ => {}
                    }
                    self.pos += 1;
                    self.push(Tok::Punct(char::from(b)), start, line);
                }
            }
        }
    }

    fn skip_line(&mut self) {
        while self.peek(0).is_some_and(|b| b != b'\n') {
            self.pos += 1;
        }
    }

    fn block_comment(&mut self) {
        self.pos += 2;
        while let Some(b) = self.peek(0) {
            if b == b'*' && self.peek(1) == Some(b'/') {
                self.pos += 2;
                return;
            }
            if b == b'\n' {
                self.line += 1;
            }
            self.pos += 1;
        }
    }

    /// A quoted string: its text, and its interpolations lexed as code.
    fn string(&mut self, nest: usize) {
        let (start, line) = (self.pos, self.line);
        self.pos += 1;
        let mut text = String::new();
        while let Some(b) = self.peek(0) {
            match b {
                b'"' => {
                    self.pos += 1;
                    break;
                }
                b'\n' => break, // HCL strings end at the line; the next line is code again
                b'\\' => {
                    let esc = self.peek(1);
                    text.push(match esc {
                        Some(b'n') => '\n',
                        Some(b't') => '\t',
                        Some(c) if c.is_ascii() => char::from(c),
                        _ => '\\',
                    });
                    self.pos += if esc.is_some_and(|c| c.is_ascii()) {
                        2
                    } else {
                        1
                    };
                }
                b'$' | b'%' if self.peek(1) == Some(b'{') => self.interp(&mut text, nest),
                _ => self.text_char(&mut text),
            }
        }
        self.push(Tok::Str(text), start, line);
    }

    /// Copies one character (whole, however many bytes) into `text`.
    fn text_char(&mut self, text: &mut String) {
        if let Some(c) = self.src.get(self.pos..).and_then(|s| s.chars().next()) {
            text.push(c);
            self.pos += c.len_utf8();
        } else {
            self.pos += 1;
        }
    }

    /// `${…}` in a string: kept in the text as written, and its inside lexed as code.
    fn interp(&mut self, text: &mut String, nest: usize) {
        let from = self.pos;
        self.pos += 2;
        if nest < MAX_NEST {
            self.code(true, nest + 1);
        } else {
            while self.peek(0).is_some_and(|b| b != b'}' && b != b'\n') {
                self.pos += 1;
            }
            self.pos += usize::from(self.peek(0) == Some(b'}'));
        }
        text.push_str(self.src.get(from..self.pos).unwrap_or(""));
    }

    /// `<<EOT` or `<<-EOT`, its lines, and the line that ends it. `false` when `<<` starts no heredoc.
    fn heredoc(&mut self, nest: usize) -> bool {
        let rest = self.src.get(self.pos + 2..).unwrap_or("");
        let rest = rest.strip_prefix('-').unwrap_or(rest);
        let tag: &str = rest
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .next()
            .unwrap_or("");
        let Some(after) = rest.get(tag.len()..) else {
            return false;
        };
        if tag.is_empty() || !after.starts_with('\n') {
            return false;
        }
        let (start, line) = (self.pos, self.line);
        self.pos = self.src.len() - after.len() + 1;
        self.line += 1;
        let mut text = String::new();
        while self.peek(0).is_some() {
            let end = self
                .src
                .get(self.pos..)
                .and_then(|s| s.find('\n'))
                .map_or(self.src.len(), |i| self.pos + i);
            if self.src.get(self.pos..end).is_some_and(|l| l.trim() == tag) {
                self.pos = end;
                break;
            }
            while self.pos < end {
                if matches!(self.peek(0), Some(b'$' | b'%')) && self.peek(1) == Some(b'{') {
                    self.interp(&mut text, nest);
                } else {
                    self.text_char(&mut text);
                }
            }
            if self.peek(0) == Some(b'\n') {
                text.push('\n');
                self.pos += 1;
                self.line += 1;
            }
        }
        self.push(Tok::Str(text), start, line);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{Tok, lex};

    fn words(src: &str) -> Vec<String> {
        lex(src)
            .into_iter()
            .filter_map(|t| match t.tok {
                Tok::Word(w) => Some(w.to_owned()),
                Tok::Str(s) => Some(format!("{s:?}")),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn strings_hide_words_but_interpolations_do_not() {
        assert_eq!(
            words(r#"a = "app.web ${module.db.url}" # app.x"#),
            ["a", "module", "db", "url", "\"app.web ${module.db.url}\""]
        );
    }

    #[test]
    fn heredocs_and_comments() {
        let src = "x = <<-EOT\n  hi ${app.api.url}\n  EOT\n/* app.no */ y = 1\n";
        assert_eq!(
            words(src),
            [
                "x",
                "app",
                "api",
                "url",
                "\"  hi ${app.api.url}\\n\"",
                "y",
                "1"
            ]
        );
    }

    #[test]
    fn lines_are_counted() {
        let t = lex("a\n\"b\"\n  c");
        let lines: Vec<usize> = t.iter().map(|t| t.line).collect();
        assert_eq!(lines, [1, 1, 2, 2, 3]);
    }
}
