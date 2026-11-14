//! Terms: parentheses, literals, names, function calls, templates, unary operators, tuple and
//! object constructors (`parseExpressionTerm`, `finishParsingFunctionCall`, `parseTupleCons`,
//! `parseObjectCons`).

use super::expr::has_errors;
use super::{Ds, Parser, between, err};
use crate::diag::Range;
use crate::lex::{Kind, Token};

impl Parser<'_> {
    pub(crate) fn term(&mut self) -> (Range, Ds) {
        let start = self.peek();
        match start.kind {
            Kind::OParen => self.parens(start),
            Kind::NumberLit => {
                self.read();
                (start.range, self.number_value(&start))
            }
            Kind::Ident => {
                self.read();
                match self.peek().kind {
                    Kind::OParen | Kind::DoubleColon => self.call(start),
                    _ => (start.range, Ds::new()),
                }
            }
            Kind::OQuote | Kind::OHeredoc => {
                let open = self.read();
                let ds = self.template(open);
                (between(open.range, self.prev_range()), ds)
            }
            Kind::Minus | Kind::Bang => {
                self.read();
                let (operand, ds) = self.with_traversals();
                (between(start.range, operand), ds)
            }
            Kind::OBrack => self.tuple(),
            Kind::OBrace => self.object(),
            _ => {
                let mut ds = Ds::new();
                if !self.recovery {
                    ds.push(if start.kind == Kind::Eof {
                        err("Missing expression", "Expected the start of an expression, but found the end of the file.", start.range)
                    } else {
                        err("Invalid expression", "Expected the start of an expression, but found an invalid expression token.", start.range)
                    });
                }
                self.set_recovery();
                (start.range, ds)
            }
        }
    }

    fn parens(&mut self, open: Token) -> (Range, Ds) {
        self.read();
        self.push_newlines(false);
        let (inner, mut ds) = self.expression();
        if has_errors(&ds) {
            self.recover(Kind::CParen);
            self.pop_newlines();
            return (inner, ds);
        }
        let close = self.peek();
        if close.kind != Kind::CParen {
            ds.push(
                err(
                    "Unbalanced parentheses",
                    "Expected a closing parenthesis to terminate the expression.",
                    close.range,
                )
                .with_context(between(open.range, close.range)),
            );
            self.set_recovery();
        }
        let close = self.read();
        self.pop_newlines();
        (between(open.range, close.range), ds)
    }

    fn call(&mut self, name: Token) -> (Range, Ds) {
        let mut ds = Ds::new();
        let mut open = self.read();
        while open.kind == Kind::DoubleColon {
            let next = self.read();
            if next.kind != Kind::Ident {
                ds.push(
                    err("Missing function name", "Function scope resolution symbol :: must be followed by a function name in this scope.", next.range)
                        .with_context(between(name.range, next.range)),
                );
                self.recover_over(Kind::OParen);
                return (between(name.range, next.range), ds);
            }
            open = self.read();
        }
        if open.kind != Kind::OParen {
            ds.push(
                err("Missing open parenthesis", "Function selector must be followed by an open parenthesis to begin the function call.", open.range)
                    .with_context(between(name.range, open.range)),
            );
            self.recover_over(Kind::OParen);
            return (between(name.range, open.range), ds);
        }
        self.push_newlines(false);
        let close = self.call_args(name, open, &mut ds);
        self.pop_newlines();
        (
            between(name.range, close.map_or_else(Range::default, |c| c.range)),
            ds,
        )
    }

    /// The arguments up to the closing parenthesis, which is `None` when recovery found none.
    fn call_args(&mut self, name: Token, open: Token, ds: &mut Ds) -> Option<Token> {
        loop {
            if self.peek().kind == Kind::CParen {
                return Some(self.read());
            }
            let (_, d) = self.expression();
            let failed = has_errors(&d);
            ds.extend(d);
            if self.recovery && failed {
                let t = self.recover(Kind::CParen);
                return (t.kind == Kind::CParen).then_some(t);
            }
            let sep = self.read();
            match sep.kind {
                Kind::CParen => return Some(sep),
                Kind::Ellipsis => {
                    if self.peek().kind == Kind::CParen {
                        return Some(self.read());
                    }
                    if !self.recovery {
                        ds.push(
                            err("Missing closing parenthesis", "An expanded function argument (with ...) must be immediately followed by closing parentheses.", sep.range)
                                .with_context(between(name.range, sep.range)),
                        );
                    }
                    return Some(self.recover(Kind::CParen));
                }
                Kind::Comma => {}
                Kind::Eof => {
                    ds.push(err("Unterminated function call", "There is no closing parenthesis for this function call before the end of the file. This may be caused by incorrect parenthesis nesting elsewhere in this file.", between(name.range, open.range)));
                    return Some(self.recover(Kind::CParen));
                }
                _ => {
                    ds.push(
                        err(
                            "Missing argument separator",
                            "A comma is required to separate each function argument from the next.",
                            sep.range,
                        )
                        .with_context(between(name.range, sep.range)),
                    );
                    return Some(self.recover(Kind::CParen));
                }
            }
            if self.peek().kind == Kind::CParen {
                return Some(self.read());
            }
        }
    }

    fn tuple(&mut self) -> (Range, Ds) {
        let open = self.read();
        self.push_newlines(false);
        let out = if self.is_keyword(&self.peek(), "for") {
            self.for_expr(open)
        } else {
            let mut ds = Ds::new();
            let close = self.tuple_items(open, &mut ds);
            (between(open.range, close.range), ds)
        };
        self.pop_newlines();
        out
    }

    fn tuple_items(&mut self, open: Token, ds: &mut Ds) -> Token {
        loop {
            if self.peek().kind == Kind::CBrack {
                return self.read();
            }
            let (_, d) = self.expression();
            let failed = has_errors(&d);
            ds.extend(d);
            if self.recovery && failed {
                return self.recover(Kind::CBrack);
            }
            let next = self.peek();
            match next.kind {
                Kind::CBrack => return self.read(),
                Kind::Comma => {
                    self.read();
                }
                _ => {
                    if !self.recovery {
                        ds.push(if next.kind == Kind::Eof {
                            err("Unterminated tuple constructor expression", "There is no corresponding closing bracket before the end of the file. This may be caused by incorrect bracket nesting elsewhere in this file.", open.range)
                        } else {
                            err("Missing item separator", "Expected a comma to mark the beginning of the next item.", next.range)
                                .with_context(between(open.range, next.range))
                        });
                    }
                    return self.recover(Kind::CBrack);
                }
            }
        }
    }

    fn object(&mut self) -> (Range, Ds) {
        let open = self.read();
        self.push_newlines(false);
        let is_for = self.is_keyword(&self.peek(), "for");
        self.pop_newlines();
        if is_for {
            return self.for_expr(open);
        }
        self.push_newlines(true);
        let mut ds = Ds::new();
        let close = self.object_items(open, &mut ds);
        self.pop_newlines();
        (between(open.range, close.range), ds)
    }
}
