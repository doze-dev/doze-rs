use super::ast::{Error, syntax};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Tok {
    /// A bare word: an attribute or a keyword.
    Word(String),
    /// `"..."`: always an attribute or table, never a keyword.
    Quoted(String),
    Str(String),
    Num(String),
    Param,
    Sym(&'static str),
}

/// The two-character symbols, tried before the one-character ones.
const SYMBOLS2: [&str; 6] = ["<<", ">>", "<=", ">=", "<>", "!="];
const SYMBOLS1: [&str; 17] = [
    ",", ".", "(", ")", "[", "]", "{", "}", ":", "*", "+", "-", ";", "=", "<", ">", "?",
];

/// The text as tokens.
struct Lexer {
    chars: Vec<char>,
    at: usize,
}

impl Lexer {
    fn at(&self, i: usize) -> Option<char> {
        self.chars.get(i).copied()
    }

    fn take_while(&mut self, keep: impl Fn(char) -> bool) -> String {
        let start = self.at;
        while self.at(self.at).is_some_and(&keep) {
            self.at += 1;
        }
        self.chars
            .get(start..self.at)
            .unwrap_or_default()
            .iter()
            .collect()
    }

    /// `12`, `1.5`, `.5`, `2e-3`. Whether it is a number DynamoDB takes is the item's check.
    fn number(&mut self) -> String {
        let mut n = self.take_while(|c| c.is_ascii_digit());
        if self.at(self.at) == Some('.') {
            self.at += 1;
            n.push('.');
            n.push_str(&self.take_while(|c| c.is_ascii_digit()));
        }
        if matches!(self.at(self.at), Some('e' | 'E')) {
            let sign = matches!(self.at(self.at + 1), Some('+' | '-'));
            let digit_at = self.at + 1 + usize::from(sign);
            if self.at(digit_at).is_some_and(|c| c.is_ascii_digit()) {
                n.extend(self.chars.get(self.at..digit_at).unwrap_or_default());
                self.at = digit_at;
                n.push_str(&self.take_while(|c| c.is_ascii_digit()));
            }
        }
        n
    }

    /// Text between `quote`s, a doubled quote standing for itself.
    fn quoted(&mut self, quote: char) -> Result<String, Error> {
        let mut s = String::new();
        self.at += 1;
        loop {
            match self.at(self.at) {
                None => return syntax("an unterminated quoted text"),
                Some(q) if q == quote && self.at(self.at + 1) == Some(quote) => {
                    s.push(quote);
                    self.at += 2;
                }
                Some(q) if q == quote => {
                    self.at += 1;
                    return Ok(s);
                }
                Some(other) => {
                    s.push(other);
                    self.at += 1;
                }
            }
        }
    }

    fn symbol(&mut self) -> Result<Tok, Error> {
        let two: String = self
            .chars
            .get(self.at..self.at + 2)
            .unwrap_or_default()
            .iter()
            .collect();
        if let Some(s) = SYMBOLS2.into_iter().find(|s| *s == two) {
            self.at += 2;
            return Ok(Tok::Sym(s));
        }
        let c = self.at(self.at).unwrap_or_default();
        let one = c.to_string();
        let s = SYMBOLS1
            .into_iter()
            .find(|s| *s == one)
            .map_or_else(|| syntax(format!("unexpected character '{c}'")), Ok)?;
        self.at += 1;
        Ok(if s == "?" { Tok::Param } else { Tok::Sym(s) })
    }

    fn next(&mut self) -> Result<Option<Tok>, Error> {
        loop {
            let Some(c) = self.at(self.at) else {
                return Ok(None);
            };
            if c.is_whitespace() {
                self.at += 1;
            } else if c == '-' && self.at(self.at + 1) == Some('-') {
                self.take_while(|c| c != '\n');
            } else {
                break;
            }
        }
        let c = self.at(self.at).unwrap_or_default();
        let tok = if c.is_ascii_alphabetic() || c == '_' {
            Tok::Word(self.take_while(|c| c.is_ascii_alphanumeric() || c == '_'))
        } else if c.is_ascii_digit()
            || (c == '.' && self.at(self.at + 1).is_some_and(|d| d.is_ascii_digit()))
        {
            Tok::Num(self.number())
        } else if c == '"' {
            Tok::Quoted(self.quoted('"')?)
        } else if c == '\'' {
            Tok::Str(self.quoted('\'')?)
        } else {
            self.symbol()?
        };
        Ok(Some(tok))
    }
}

pub(super) fn lex(text: &str) -> Result<Vec<Tok>, Error> {
    let mut lexer = Lexer {
        chars: text.chars().collect(),
        at: 0,
    };
    let mut out = Vec::new();
    while let Some(tok) = lexer.next()? {
        out.push(tok);
    }
    Ok(out)
}

pub(super) fn describe(t: &Tok) -> String {
    match t {
        Tok::Word(w) => format!("'{w}'"),
        Tok::Quoted(q) => format!("\"{q}\""),
        Tok::Str(s) => format!("'{s}'"),
        Tok::Num(n) => n.clone(),
        Tok::Param => "?".to_owned(),
        Tok::Sym(s) => format!("'{s}'"),
    }
}
