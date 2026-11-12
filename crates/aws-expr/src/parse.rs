//! The parser's machinery: tokens, document paths and operands. The grammar of each expression kind is in `parse_cond` and
//! `parse_update`.

use crate::Invalid;
use crate::ast::{Elem, Operand, Path};
use crate::lex::{Tok, Token, tokens};
use crate::reserved::is_reserved;
use crate::scope::{Kind, Scope};

/// The longest expression AWS reads. `(memory)`: the developer guide gives 4 KB.
const MAX_LEN: usize = 4096;

/// A parser over one expression's text.
pub(crate) struct Parser<'s, 'a> {
    pub(crate) text: &'s str,
    toks: Vec<Token>,
    at: usize,
    pub(crate) kind: Kind,
    pub(crate) scope: &'s mut Scope<'a>,
    /// The first error that is not a syntax error (an undefined name, a reserved word): reported only if the text parses, as AWS
    /// reads the whole expression before it resolves what it names.
    pending: Option<Invalid>,
}

impl<'s, 'a> Parser<'s, 'a> {
    pub(crate) fn new(
        text: &'s str,
        kind: Kind,
        scope: &'s mut Scope<'a>,
    ) -> Result<Self, Invalid> {
        if text.len() > MAX_LEN {
            return Err(kind.invalid(&format!(
                "Expression size has exceeded the maximum allowed size; size: {}",
                text.len()
            )));
        }
        Ok(Self {
            text,
            toks: tokens(text),
            at: 0,
            kind,
            scope,
            pending: None,
        })
    }

    pub(crate) fn peek(&self) -> Tok {
        self.nth(0)
    }

    pub(crate) fn nth(&self, ahead: usize) -> Tok {
        self.toks.get(self.at + ahead).map_or(Tok::Eof, |t| t.tok)
    }

    pub(crate) fn bump(&mut self) -> Token {
        let t = self.toks.get(self.at).copied().unwrap_or(Token {
            tok: Tok::Eof,
            start: self.text.len(),
            end: self.text.len(),
        });
        if t.tok != Tok::Eof {
            self.at += 1;
        }
        t
    }

    /// The text of the next token.
    pub(crate) fn peek_text(&self) -> &'s str {
        self.toks
            .get(self.at)
            .and_then(|t| self.text.get(t.start..t.end))
            .unwrap_or_default()
    }

    /// Whether the next token is the bare word `word`, in any case.
    pub(crate) fn at_word(&self, word: &str) -> bool {
        self.peek() == Tok::Word && self.peek_text().eq_ignore_ascii_case(word)
    }

    /// Takes the next token if it is `tok`.
    pub(crate) fn eat(&mut self, tok: Tok) -> bool {
        let yes = self.peek() == tok;
        if yes {
            self.bump();
        }
        yes
    }

    /// Takes the next token, which must be `tok`.
    pub(crate) fn expect(&mut self, tok: Tok) -> Result<Token, Invalid> {
        if self.peek() == tok {
            Ok(self.bump())
        } else {
            Err(self.syntax())
        }
    }

    /// AWS's syntax error: the offending token, and the token before it with it (`near`).
    pub(crate) fn syntax(&self) -> Invalid {
        let tok = self.toks.get(self.at).copied();
        let shown = |t: Token| self.text.get(t.start..t.end).unwrap_or_default().to_owned();
        let (token, near) = match tok {
            Some(t) if t.tok != Tok::Eof => {
                let prev = self
                    .at
                    .checked_sub(1)
                    .and_then(|p| self.toks.get(p))
                    .copied();
                let near = match prev {
                    Some(p) => format!("{} {}", shown(p), shown(t)),
                    None => shown(t),
                };
                (shown(t), near)
            }
            _ => {
                let prev = self
                    .at
                    .checked_sub(1)
                    .and_then(|p| self.toks.get(p))
                    .copied();
                ("<EOF>".to_owned(), prev.map(shown).unwrap_or_default())
            }
        };
        self.kind.invalid(&format!(
            "Syntax error; token: \"{token}\", near: \"{near}\""
        ))
    }

    /// Remembers an error that waits for the rest of the text to parse.
    pub(crate) fn defer(&mut self, e: Invalid) {
        if self.pending.is_none() {
            self.pending = Some(e);
        }
    }

    /// Ends the parse: the text must be used up, and no deferred error may be waiting.
    pub(crate) fn finish(self) -> Result<(), Invalid> {
        if self.peek() != Tok::Eof {
            return Err(self.syntax());
        }
        self.pending.map_or(Ok(()), Err)
    }

    /// A reference to a name or a value, resolved: a placeholder that was not supplied is deferred.
    pub(crate) fn name_ref(&mut self, reference: &str) -> String {
        self.scope.name(reference).unwrap_or_else(|| {
            let msg = format!("An expression attribute name used in the document path is not defined; attribute name: {reference}");
            self.defer(self.kind.invalid(&msg));
            String::new()
        })
    }

    pub(crate) fn value_ref(&mut self, reference: &str) -> crate::Value {
        self.scope.value(reference).unwrap_or_else(|| {
            let msg = format!("An expression attribute value used in expression is not defined; attribute value: {reference}");
            self.defer(self.kind.invalid(&msg));
            crate::Value::Null
        })
    }

    /// One name in a path: a bare word (which must not be reserved) or `#name`.
    fn path_name(&mut self) -> Result<String, Invalid> {
        match self.peek() {
            Tok::NameRef => {
                let t = self.peek_text();
                self.bump();
                Ok(self.name_ref(t))
            }
            Tok::Word => {
                let t = self.peek_text();
                self.bump();
                if is_reserved(t) {
                    let msg =
                        format!("Attribute name is a reserved keyword; reserved keyword: {t}");
                    self.defer(self.kind.invalid(&msg));
                }
                Ok(t.to_owned())
            }
            _ => Err(self.syntax()),
        }
    }

    /// A document path: `a`, `#a.b[2][0].c`.
    pub(crate) fn path(&mut self) -> Result<Path, Invalid> {
        let mut elems = vec![Elem::Key(self.path_name()?)];
        loop {
            if self.eat(Tok::Dot) {
                elems.push(Elem::Key(self.path_name()?));
            } else if self.eat(Tok::LBracket) {
                let t = self.peek_text();
                self.expect(Tok::Int)?;
                let n = t.parse::<usize>().map_err(|_| self.syntax())?;
                self.expect(Tok::RBracket)?;
                elems.push(Elem::Index(n));
            } else {
                return Ok(Path(elems));
            }
        }
    }

    /// An operand of a condition or an update: a value, a path, `size(path)`, and in an update `if_not_exists` and `list_append`.
    pub(crate) fn operand(&mut self) -> Result<Operand, Invalid> {
        match self.peek() {
            Tok::ValueRef => {
                let t = self.peek_text();
                self.bump();
                Ok(Operand::Value(self.value_ref(t)))
            }
            Tok::Word if self.nth(1) == Tok::LParen => self.call(),
            Tok::Word
                if matches!(
                    self.peek_text().to_ascii_uppercase().as_str(),
                    "AND" | "OR" | "BETWEEN" | "IN"
                ) =>
            {
                Err(self.syntax())
            }
            Tok::Word | Tok::NameRef => self.path().map(Operand::Path),
            _ => Err(self.syntax()),
        }
    }

    /// `size(path)`, `if_not_exists(path, operand)`, `list_append(a, b)`.
    fn call(&mut self) -> Result<Operand, Invalid> {
        let name = self.peek_text();
        self.bump();
        self.expect(Tok::LParen)?;
        let found = match (name, self.kind) {
            ("size", _) => Operand::Size(self.path()?),
            ("if_not_exists", Kind::Update) => {
                let path = self.path()?;
                self.expect(Tok::Comma)?;
                Operand::IfNotExists(path, Box::new(self.operand()?))
            }
            ("list_append", Kind::Update) => {
                let a = self.operand()?;
                self.expect(Tok::Comma)?;
                Operand::ListAppend(Box::new(a), Box::new(self.operand()?))
            }
            _ => {
                return Err(self
                    .kind
                    .invalid(&format!("Invalid function name; function: {name}")));
            }
        };
        self.expect(Tok::RParen)?;
        Ok(found)
    }
}
