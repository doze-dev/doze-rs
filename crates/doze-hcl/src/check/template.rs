//! Templates (`parseTemplateParts`, then `templateParser`'s pass over the directives): the parts
//! first, with their own diagnostics, then whether the `if`/`for`/`else`/`end` directives balance.

use super::{Ds, Parser, between, err, place};
use crate::diag::Range;
use crate::lex::{Kind, Token};

/// What the directive pass sees of each part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Other,
    If(Range),
    For(Range),
    Else(Range),
    EndIf(Range),
    EndFor(Range),
    End(Range),
}

impl Part {
    fn ctrl_name(self) -> Option<(&'static str, Range)> {
        match self {
            Part::Else(r) => Some(("else", r)),
            Part::EndIf(r) => Some(("endif", r)),
            Part::EndFor(r) => Some(("endfor", r)),
            _ => None,
        }
    }
}

impl Parser<'_> {
    /// The template after `open` (a quote or heredoc opener), up to its closer.
    pub(crate) fn template(&mut self, open: Token) -> Ds {
        let end = if open.kind == Kind::OQuote {
            Kind::CQuote
        } else {
            Kind::CHeredoc
        };
        let (parts, mut ds) = self.template_parts(end);
        ds.extend(directives(&parts, self.file));
        ds
    }

    fn template_parts(&mut self, end: Kind) -> (Vec<Part>, Ds) {
        let mut parts = Vec::new();
        let mut ds = Ds::new();
        let start = self.next_range();
        let end_range = loop {
            let next = self.read();
            if next.kind == end {
                break next.range;
            }
            match next.kind {
                Kind::StringLit | Kind::QuotedLit => {
                    let quoted = next.kind == Kind::QuotedLit;
                    ds.extend(super::strlit::escapes(
                        self.bytes(&next),
                        next.range,
                        quoted,
                    ));
                    parts.push(Part::Other);
                }
                Kind::TemplateInterp => {
                    self.interpolation(start, end, &mut ds);
                    parts.push(Part::Other);
                }
                Kind::TemplateControl => {
                    if let Some(p) = self.directive(next, start, &mut ds) {
                        parts.push(p);
                    }
                }
                _ => {
                    if !self.recovery {
                        ds.push(
                            err(
                                "Unterminated template string",
                                "No closing marker was found for the string.",
                                next.range,
                            )
                            .with_context(between(start, next.range)),
                        );
                    }
                    break self.recover(end).range;
                }
            }
        };
        parts.push(Part::End(end_range));
        (parts, ds)
    }

    fn interpolation(&mut self, start: Range, end: Kind, ds: &mut Ds) {
        self.push_newlines(false);
        let (_, d) = self.expression();
        ds.extend(d);
        let close = self.peek();
        if close.kind == Kind::TemplateSeqEnd {
            self.read();
        } else {
            if !self.recovery {
                let unclosed =
                    |detail: &str| err("Unclosed template interpolation sequence", detail, start);
                let extra = |detail: &str| {
                    err(
                        "Extra characters after interpolation expression",
                        detail,
                        close.range,
                    )
                    .with_context(between(start, close.range))
                };
                ds.push(match close.kind {
                    Kind::Eof => unclosed("There is no closing brace for this interpolation sequence before the end of the file. This might be caused by incorrect nesting inside the given expression."),
                    Kind::Colon => extra("Template interpolation doesn't expect a colon at this location. Did you intend this to be a literal sequence to be processed as part of another language? If so, you can escape it by starting with \"$${\" instead of just \"${\"."),
                    Kind::CQuote | Kind::OQuote if end == Kind::CQuote => unclosed("There is no closing brace for this interpolation sequence before the end of the quoted template. This might be caused by incorrect nesting inside the given expression."),
                    _ => extra("Expected a closing brace to end the interpolation expression, but found extra characters.\n\nThis can happen when you include interpolation syntax for another language, such as shell scripting, but forget to escape the interpolation start token. If this is an embedded sequence for another language, escape it by starting with \"$${\" instead of just \"${\"."),
                });
            }
            self.recover(Kind::TemplateSeqEnd);
        }
        self.pop_newlines();
    }

    /// A `%{ … }` directive. `None` when it was refused and skipped.
    fn directive(&mut self, next: Token, start: Range, ds: &mut Ds) -> Option<Part> {
        self.push_newlines(false);
        let kw = self.peek();
        let part = if kw.kind == Kind::Ident {
            self.read();
            self.directive_keyword(next, kw, ds)
        } else {
            if !self.recovery {
                ds.push(
                    err("Invalid template directive", "A template directive keyword (\"if\", \"for\", etc) is expected at the beginning of a %{ sequence.", kw.range)
                        .with_context(between(next.range, kw.range)),
                );
            }
            None
        };
        let Some(part) = part else {
            self.recover(Kind::TemplateSeqEnd);
            self.pop_newlines();
            return None;
        };
        let close = self.peek();
        if close.kind == Kind::TemplateSeqEnd {
            self.read();
        } else {
            if !self.recovery {
                let summary = format!(
                    "Extra characters in {} marker",
                    String::from_utf8_lossy(self.bytes(&kw))
                );
                ds.push(
                    err(
                        &summary,
                        "Expected a closing brace to end the sequence, but found extra characters.",
                        close.range,
                    )
                    .with_context(between(start, close.range)),
                );
            }
            self.recover(Kind::TemplateSeqEnd);
        }
        self.pop_newlines();
        Some(part)
    }

    fn directive_keyword(&mut self, next: Token, kw: Token, ds: &mut Ds) -> Option<Part> {
        let word = self.bytes(&kw);
        let here = |p: &Self| between(next.range, p.next_range());
        match word {
            b"if" => {
                let (_, d) = self.expression();
                ds.extend(d);
                Some(Part::If(here(self)))
            }
            b"else" => Some(Part::Else(here(self))),
            b"endif" => Some(Part::EndIf(here(self))),
            b"endfor" => Some(Part::EndFor(here(self))),
            b"for" => self.for_directive(next, ds),
            _ => {
                if !self.recovery {
                    let given = String::from_utf8_lossy(word).into_owned();
                    let candidates = ["if", "for", "else", "endif", "endfor"].map(str::to_owned);
                    let hint = crate::eval::suggestion(&given, &candidates)
                        .map(|s| format!(" Did you mean {s:?}?"))
                        .unwrap_or_default();
                    let detail =
                        format!("{given:?} is not a valid template control keyword.{hint}");
                    ds.push(
                        err("Invalid template control keyword", detail, kw.range)
                            .with_context(between(next.range, kw.range)),
                    );
                }
                None
            }
        }
    }

    fn for_directive(&mut self, next: Token, ds: &mut Ds) -> Option<Part> {
        let mut bad = |p: &mut Self, detail: &str| {
            if !p.recovery {
                ds.push(err("Invalid 'for' directive", detail, p.next_range()));
            }
            None
        };
        if self.peek().kind != Kind::Ident {
            return bad(self, "For directive requires variable name after 'for'.");
        }
        self.read();
        if self.peek().kind == Kind::Comma {
            self.read();
            if self.peek().kind != Kind::Ident {
                return bad(
                    self,
                    "For directive requires value variable name after comma.",
                );
            }
            self.read();
        }
        if !self.is_keyword(&self.peek(), "in") {
            return bad(self, "For directive requires 'in' keyword after names.");
        }
        self.read();
        let (_, d) = self.expression();
        ds.extend(d);
        Some(Part::For(between(next.range, self.next_range())))
    }
}

/// `templateParser.parseRoot`: directives that do not balance.
fn directives(parts: &[Part], file: &str) -> Ds {
    let mut ds = Ds::new();
    let mut i = 0;
    while let Some(p) = parts.get(i) {
        match *p {
            Part::End(_) => break,
            Part::If(_) | Part::For(_) => i = nested(parts, i, file, &mut ds),
            other => {
                i += 1;
                if let Some((name, at)) = other.ctrl_name() {
                    ds.push(err(
                        &format!("Unexpected {name} directive"),
                        "The control directives within this template are unbalanced.",
                        at,
                    ));
                }
            }
        }
    }
    ds
}

/// `parseIf` or `parseFor` from `parts[i]`; returns the index after it.
fn nested(parts: &[Part], mut i: usize, file: &str, ds: &mut Ds) -> usize {
    let (is_if, open) = match parts.get(i) {
        Some(Part::If(r)) => (true, *r),
        Some(Part::For(r)) => (false, *r),
        _ => return i + 1,
    };
    i += 1;
    let mut in_else = false;
    loop {
        match parts.get(i).copied() {
            None => return i,
            Some(Part::End(at)) => {
                let (what, closer) = if is_if {
                    ("if", "endif")
                } else {
                    ("for", "endfor")
                };
                let detail = format!(
                    "The {what} directive at {} is missing its corresponding {closer} directive.",
                    place(file, open)
                );
                ds.push(err("Unexpected end of template", detail, at));
                return i;
            }
            Some(Part::If(_) | Part::For(_)) => i = nested(parts, i, file, ds),
            Some(Part::Other) => i += 1,
            Some(ctrl) => {
                i += 1;
                let Some((name, at)) = ctrl.ctrl_name() else {
                    continue;
                };
                match (is_if, name) {
                    (true, "else") if !in_else => {
                        in_else = true;
                        continue;
                    }
                    (true, "endif") | (false, "endfor") => return i,
                    (true, "else") => ds.push(err("Unexpected else directive", format!("Already in the else clause for the if started at {}.", place(file, open)), at)),
                    (false, "else") => ds.push(err("Unexpected else directive", "An else clause is not expected for a for directive.", at)),
                    (true, _) => ds.push(err(&format!("Unexpected {name} directive"), format!("Expecting an endif directive for the if started at {}.", place(file, open)), at)),
                    (false, _) => ds.push(err(&format!("Unexpected {name} directive"), format!("Expecting an endfor directive corresponding to the for directive at {}.", place(file, open)), at)),
                }
                return i;
            }
        }
    }
}
