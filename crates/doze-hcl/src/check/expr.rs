//! Expressions (`parseTernaryConditional`, `parseBinaryOps`, `parseExpressionTraversals`,
//! `parseExpressionTerm`). Each returns the expression's range as Go's tree would give it, because
//! later diagnostics point at it.

use super::{Ds, Parser, between, err};
use crate::diag::{Range, Severity};
use crate::lex::Kind;

/// Go's `binaryOps`, lowest precedence first.
const LEVELS: [&[Kind]; 6] = [
    &[Kind::Or],
    &[Kind::And],
    &[Kind::EqualOp, Kind::NotEqual],
    &[
        Kind::GreaterThan,
        Kind::GreaterThanEq,
        Kind::LessThan,
        Kind::LessThanEq,
    ],
    &[Kind::Plus, Kind::Minus],
    &[Kind::Star, Kind::Slash, Kind::Percent],
];

pub(crate) fn has_errors(ds: &Ds) -> bool {
    ds.iter().any(|d| d.severity == Severity::Error)
}

impl Parser<'_> {
    pub(crate) fn expression(&mut self) -> (Range, Ds) {
        let mut ds = Ds::new();
        if !self.enter(&mut ds) {
            return (self.next_range(), ds);
        }
        let out = self.conditional(ds);
        self.leave();
        out
    }

    fn conditional(&mut self, mut ds: Ds) -> (Range, Ds) {
        let start = self.next_range();
        let (cond, d) = self.binary(0);
        let failed = has_errors(&d);
        ds.extend(d);
        if self.recovery && failed || self.peek().kind != Kind::Question {
            return (cond, ds);
        }
        self.read();
        let (_, d) = self.expression();
        let failed = has_errors(&d);
        ds.extend(d);
        if self.recovery && failed {
            return (cond, ds);
        }
        let colon = self.peek();
        if colon.kind != Kind::Colon {
            ds.push(
                err(
                    "Missing false expression in conditional",
                    "The conditional operator (...?...:...) requires a false expression, delimited by a colon.",
                    colon.range,
                )
                .with_context(between(start, colon.range)),
            );
            return (cond, ds);
        }
        self.read();
        let (f, d) = self.expression();
        let failed = has_errors(&d);
        ds.extend(d);
        if self.recovery && failed {
            return (cond, ds);
        }
        (between(start, f), ds)
    }

    fn binary(&mut self, level: usize) -> (Range, Ds) {
        let Some(ops) = LEVELS.get(level) else {
            return self.with_traversals();
        };
        let (mut lhs, mut ds) = self.binary(level + 1);
        if self.recovery && has_errors(&ds) {
            return (lhs, ds);
        }
        let mut rhs: Option<Range> = None;
        while ops.contains(&self.peek().kind) {
            if let Some(r) = rhs {
                lhs = between(lhs, r);
            }
            self.read();
            let (r, d) = self.binary(level + 1);
            let failed = has_errors(&d);
            ds.extend(d);
            if self.recovery && failed {
                return (lhs, ds);
            }
            rhs = Some(r);
        }
        match rhs {
            Some(r) => (between(lhs, r), ds),
            None => (lhs, ds),
        }
    }

    pub(crate) fn with_traversals(&mut self) -> (Range, Ds) {
        let (term, mut ds) = self.term();
        let (r, d) = self.traversals(term);
        ds.extend(d);
        (r, ds)
    }

    /// Attribute access, legacy `.0` indexes, splats and index brackets after `from`.
    fn traversals(&mut self, from: Range) -> (Range, Ds) {
        let mut ds = Ds::new();
        let mut ret = from;
        loop {
            match self.peek().kind {
                Kind::Dot => ret = self.dot_step(from, ret, &mut ds),
                Kind::OBrack => ret = self.bracket_step(from, &mut ds),
                _ => return (ret, ds),
            }
        }
    }

    fn legacy_index(&mut self, ds: &mut Ds, splat: bool) -> Range {
        let num = self.read();
        let text = self.bytes(&num);
        if let Some(i) = text.iter().position(|b| *b == b'.') {
            let (a, b) = (
                String::from_utf8_lossy(text.get(..i).unwrap_or_default()),
                String::from_utf8_lossy(text.get(i + 1..).unwrap_or_default()),
            );
            let how = if splat {
                "the proper index syntax with a full splat expression [*] instead"
            } else {
                "the proper index syntax instead"
            };
            let detail = format!(
                "When using the legacy index syntax, chaining two indexes together is not permitted. Use {how}, like [{a}][{b}]."
            );
            ds.push(err("Invalid legacy index syntax", detail, num.range));
        } else {
            ds.extend(self.number_value(&num));
        }
        num.range
    }

    fn dot_step(&mut self, from: Range, ret: Range, ds: &mut Ds) -> Range {
        let dot = self.read();
        let attr = self.peek();
        match attr.kind {
            Kind::Ident => between(ret, self.read().range),
            Kind::NumberLit => {
                let r = self.legacy_index(ds, false);
                between(ret, r)
            }
            Kind::Star => self.attr_splat(from, ret, attr.range, ds),
            _ => {
                ds.push(err(
                    "Invalid attribute name",
                    "An attribute name is required after a dot.",
                    attr.range,
                ));
                self.set_recovery();
                between(from, dot.range)
            }
        }
    }

    /// `.*` and the attribute steps after it.
    fn attr_splat(&mut self, from: Range, ret: Range, attr_at: Range, ds: &mut Ds) -> Range {
        let marker = self.read();
        let mut last = marker.range;
        while self.peek().kind == Kind::Dot {
            self.read();
            match self.peek().kind {
                Kind::NumberLit => {
                    // Go reports a chained legacy index here at the star, not at the number.
                    let num = self.peek();
                    if self.bytes(&num).contains(&b'.') {
                        self.read();
                        let text = String::from_utf8_lossy(self.bytes(&num)).into_owned();
                        let (a, b) = text.split_once('.').unwrap_or_default();
                        let detail = format!(
                            "When using the legacy index syntax, chaining two indexes together is not permitted. Use the proper index syntax with a full splat expression [*] instead, like [{a}][{b}]."
                        );
                        ds.push(err("Invalid legacy index syntax", detail, attr_at));
                        last = num.range;
                    } else {
                        last = self.legacy_index(ds, true);
                    }
                }
                Kind::Ident => last = self.read().range,
                k => {
                    if !self.recovery {
                        ds.push(if k == Kind::Star {
                            err("Nested splat expression not allowed", "A splat expression (*) cannot be used inside another attribute-only splat expression.", self.next_range())
                        } else {
                            err("Invalid attribute name", "An attribute name is required after a dot.", attr_at)
                        });
                    }
                    self.set_recovery();
                    // Go goes on with the outer traversal loop from here, with no new step.
                    return ret;
                }
            }
        }
        between(from, last)
    }

    fn bracket_step(&mut self, from: Range, ds: &mut Ds) -> Range {
        let open = self.read();
        if self.peek().kind == Kind::Star {
            self.read();
            let mut close = self.read();
            if close.kind != Kind::CBrack && !self.recovery {
                // Go points its diagnostic at its `close` variable, which recovery then
                // overwrites: the subject is where recovery stopped.
                close = self.recover(Kind::CBrack);
                ds.push(err(
                    "Missing close bracket on splat index",
                    "The star for a full splat operator must be immediately followed by a closing bracket (\"]\").",
                    close.range,
                ));
            }
            let (each, d) = self.traversals(between(open.range, close.range));
            ds.extend(d);
            return between(from, each);
        }
        self.push_newlines(false);
        let (_, d) = self.expression();
        let failed = has_errors(&d);
        ds.extend(d);
        let close = if self.recovery && failed {
            self.recover(Kind::CBrack)
        } else {
            let close = self.read();
            if close.kind != Kind::CBrack && !self.recovery {
                // As above: the subject is where recovery stopped (Go aliases `close`).
                let close = self.recover(Kind::CBrack);
                ds.push(err(
                    "Missing close bracket on index",
                    "The index operator must end with a closing bracket (\"]\").",
                    close.range,
                ));
                close
            } else {
                close
            }
        };
        self.pop_newlines();
        between(from, close.range)
    }

    pub(crate) fn number_value(&self, tok: &crate::lex::Token) -> Ds {
        let text = String::from_utf8_lossy(self.bytes(tok));
        if crate::number::Number::parse(&text).is_some() {
            return Ds::new();
        }
        vec![err(
            "Invalid number literal",
            "Failed to recognize the value of this number literal.",
            tok.range,
        )]
    }
}
