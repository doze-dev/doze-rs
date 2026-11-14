//! Object constructor items and `for` expressions (`parseObjectCons`, `finishParsingForExpr`).

use super::expr::has_errors;
use super::{Ds, Parser, between, err};
use crate::diag::Range;
use crate::lex::{Kind, Token};

const UNTERMINATED_OBJECT: &str = "There is no corresponding closing brace before the end of the file. This may be caused by incorrect brace nesting elsewhere in this file.";

impl Parser<'_> {
    pub(crate) fn object_items(&mut self, open: Token, ds: &mut Ds) -> Token {
        loop {
            let next = self.peek();
            match next.kind {
                Kind::Newline => {
                    self.read();
                    continue;
                }
                Kind::CBrace => return self.read(),
                _ => {}
            }
            let (_, d) = self.expression();
            let failed = has_errors(&d);
            ds.extend(d);
            if self.recovery && failed {
                return self.recover(Kind::CBrace);
            }
            let next = self.peek();
            if next.kind != Kind::Equal && next.kind != Kind::Colon {
                if !self.recovery {
                    ds.push(missing_value(open, next));
                }
                return self.recover(Kind::CBrace);
            }
            self.read();
            let (_, d) = self.expression();
            let failed = has_errors(&d);
            ds.extend(d);
            if self.recovery && failed {
                return self.recover(Kind::CBrace);
            }
            let next = self.peek();
            match next.kind {
                Kind::CBrace => return self.read(),
                Kind::Comma | Kind::Newline => {
                    self.read();
                }
                _ => {
                    if !self.recovery {
                        ds.push(if next.kind == Kind::Eof {
                            err("Unterminated object constructor expression", UNTERMINATED_OBJECT, open.range)
                        } else {
                            err("Missing attribute separator", "Expected a newline or comma to mark the beginning of the next attribute.", next.range)
                                .with_context(between(open.range, next.range))
                        });
                    }
                    return self.recover(Kind::CBrace);
                }
            }
        }
    }

    pub(crate) fn for_expr(&mut self, open: Token) -> (Range, Ds) {
        self.push_newlines(false);
        self.read(); // `for`
        let close_kind = if open.kind == Kind::OBrace {
            Kind::CBrace
        } else {
            Kind::CBrack
        };
        let mut ds = Ds::new();
        let out = match self.for_head(open, &mut ds) {
            Some(()) => self.for_body(open, close_kind, ds),
            None => {
                let close = self.recover(close_kind);
                (between(open.range, close.range), ds)
            }
        };
        self.pop_newlines();
        out
    }

    /// The names, `in` and the collection, up to the colon. `None` when recovery is needed.
    fn for_head(&mut self, open: Token, ds: &mut Ds) -> Option<()> {
        let bad = |p: &mut Self, ds: &mut Ds, detail: &str| {
            if !p.recovery {
                let at = p.next_range();
                ds.push(
                    err("Invalid 'for' expression", detail, at)
                        .with_context(between(open.range, at)),
                );
            }
        };
        if self.peek().kind != Kind::Ident {
            bad(
                self,
                ds,
                "For expression requires variable name after 'for'.",
            );
            return None;
        }
        self.read();
        if self.peek().kind == Kind::Comma {
            self.read();
            if self.peek().kind != Kind::Ident {
                bad(
                    self,
                    ds,
                    "For expression requires value variable name after comma.",
                );
                return None;
            }
            self.read();
        }
        if !self.is_keyword(&self.peek(), "in") {
            bad(
                self,
                ds,
                "For expression requires the 'in' keyword after its name declarations.",
            );
            return None;
        }
        self.read();
        let (_, d) = self.expression();
        let failed = has_errors(&d);
        ds.extend(d);
        if self.recovery && failed {
            return None;
        }
        if self.peek().kind != Kind::Colon {
            bad(
                self,
                ds,
                "For expression requires a colon after the collection expression.",
            );
            return None;
        }
        self.read();
        Some(())
    }

    fn for_body(&mut self, open: Token, close_kind: Kind, mut ds: Ds) -> (Range, Ds) {
        let (mut val, mut val_ds) = self.expression();
        let mut key: Option<(Range, Ds)> = None;
        if self.peek().kind == Kind::FatArrow {
            self.read();
            key = Some((val, val_ds));
            (val, val_ds) = self.expression();
        }
        let key_failed = key.as_ref().is_some_and(|(_, d)| has_errors(d));
        let key_range = key.as_ref().map(|(r, _)| *r);
        if let Some((_, d)) = key {
            ds.extend(d);
        }
        let val_failed = has_errors(&val_ds);
        ds.extend(val_ds);
        if self.recovery && (key_failed || val_failed) {
            let close = self.recover(close_kind);
            return (between(open.range, close.range), ds);
        }
        let ellipsis = (self.peek().kind == Kind::Ellipsis).then(|| self.read());
        if self.is_keyword(&self.peek(), "if") {
            self.read();
            let (_, d) = self.expression();
            let failed = has_errors(&d);
            ds.extend(d);
            if self.recovery && failed {
                let close = self.recover(close_kind);
                return (between(open.range, close.range), ds);
            }
        }
        let close = if self.peek().kind == close_kind {
            self.read()
        } else {
            if !self.recovery {
                let at = self.next_range();
                ds.push(
                    err(
                        "Invalid 'for' expression",
                        "Extra characters after the end of the 'for' expression.",
                        at,
                    )
                    .with_context(between(open.range, at)),
                );
            }
            self.recover(close_kind)
        };
        let whole = between(open.range, close.range);
        let shape = |detail: &str, at: Range| {
            err("Invalid 'for' expression", detail, at).with_context(whole)
        };
        if close_kind == Kind::CBrack {
            if let Some(k) = key_range {
                ds.push(shape(
                    "Key expression is not valid when building a tuple.",
                    k,
                ));
            }
            if let Some(e) = ellipsis {
                ds.push(shape(
                    "Grouping ellipsis (...) cannot be used when building a tuple.",
                    e.range,
                ));
            }
        } else if key_range.is_none() {
            ds.push(shape(
                "Key expression is required when building an object.",
                val,
            ));
        }
        (whole, ds)
    }
}

fn missing_value(open: Token, next: Token) -> crate::diag::Diagnostic {
    let ctx = between(open.range, next.range);
    match next.kind {
        Kind::Newline | Kind::Comma => err("Missing attribute value", "Expected an attribute value, introduced by an equals sign (\"=\").", next.range).with_context(ctx),
        Kind::Ident => err("Missing key/value separator", "Expected an equals sign (\"=\") to mark the beginning of the attribute value. If you intended to given an attribute name containing periods or spaces, write the name in quotes to create a string literal.", next.range).with_context(ctx),
        Kind::Eof => err("Unterminated object constructor expression", UNTERMINATED_OBJECT, open.range),
        _ => err("Missing key/value separator", "Expected an equals sign (\"=\") to mark the beginning of the attribute value.", next.range).with_context(ctx),
    }
}
