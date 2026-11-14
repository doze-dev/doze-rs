//! A template as Go's parser sees it: a flat stream of literal, interpolation and directive tokens.
//!
//! hcl-edit nests directives and dedents `<<-` heredocs while parsing. Go instead reads tokens (a
//! heredoc's literal text one line per token), trims the literal beside each `~`, then dedents a
//! `<<-` heredoc by the lines' common indentation (only lines that start with a literal count, and
//! a `~` can hide a line start). The stream is rebuilt here so the results match Go's to the byte.

use super::Evaluator;
use super::template::Part;
use hcl_edit::Span as _;
use hcl_edit::expr::Expression;
use hcl_edit::repr::Spanned;
use hcl_edit::template::{Directive, Element, ForTemplateExpr, Strip, Template};
use std::ops::Range as Span;
use unicode_segmentation::UnicodeSegmentation;

pub(super) enum Tok<'a> {
    Lit(String),
    Interp(&'a Expression),
    If(&'a Expression, Option<Span<usize>>),
    Else,
    EndIf,
    For(&'a ForTemplateExpr, Option<Span<usize>>),
    EndFor,
}

/// How a literal element becomes literal tokens.
pub(super) enum Literals<'t> {
    /// A quoted string: hcl-edit's (unescaped) text, one token.
    Quoted,
    /// A heredoc: the source text, one token per line, with `$${` and `%%{` unescaped.
    Heredoc(&'t str),
}

impl Literals<'_> {
    fn push(&self, l: &Spanned<String>, out: &mut Vec<(Tok<'_>, Strip)>) {
        let raw = match (self, l.span()) {
            (Literals::Heredoc(src), Some(span)) => src.get(span),
            _ => None,
        };
        let Some(raw) = raw else {
            out.push((Tok::Lit(l.value().clone()), Strip::None));
            return;
        };
        for line in raw.split_inclusive('\n') {
            out.push((
                Tok::Lit(line.replace("$${", "${").replace("%%{", "%{")),
                Strip::None,
            ));
        }
    }
}

pub(super) fn flatten<'a>(t: &'a Template, lits: &Literals<'_>, out: &mut Vec<(Tok<'a>, Strip)>) {
    for el in t.iter() {
        match el {
            Element::Literal(l) => lits.push(l, out),
            Element::Interpolation(i) => out.push((Tok::Interp(&i.expr), i.strip)),
            Element::Directive(d) => match d.as_ref() {
                Directive::If(d) => {
                    out.push((Tok::If(&d.if_expr.cond_expr, d.span()), d.if_expr.strip));
                    flatten(&d.if_expr.template, lits, out);
                    if let Some(e) = &d.else_expr {
                        out.push((Tok::Else, e.strip));
                        flatten(&e.template, lits, out);
                    }
                    out.push((Tok::EndIf, d.endif_expr.strip));
                }
                Directive::For(d) => {
                    out.push((Tok::For(&d.for_expr, d.span()), d.for_expr.strip));
                    flatten(&d.for_expr.template, lits, out);
                    out.push((Tok::EndFor, d.endfor_expr.strip));
                }
            },
        }
    }
}

/// Applies each `~` to the literal token right before (start) or right after (end) its marker.
pub(super) fn trim(toks: &mut [(Tok<'_>, Strip)]) {
    for i in 0..toks.len() {
        let strip = toks.get(i).map_or(Strip::None, |t| t.1);
        if strip.strip_start()
            && i > 0
            && let Some((Tok::Lit(s), _)) = toks.get_mut(i - 1)
        {
            *s = s.trim_end().to_owned();
        }
        if strip.strip_end()
            && let Some((Tok::Lit(s), _)) = toks.get_mut(i + 1)
        {
            *s = s.trim_start().to_owned();
        }
    }
}

/// Go's `flushHeredocTemplateParts`: removes the smallest indentation found at the start of the lines
/// that begin with a literal from each of those lines. A line that begins with anything else counts
/// as unindented; a blank line does not count.
pub(super) fn flush(toks: &mut [(Tok<'_>, Strip)]) {
    let mut min = usize::MAX;
    let mut newline = true;
    let mut adjust = Vec::new();
    for (i, (tok, _)) in toks.iter().enumerate() {
        if newline {
            newline = false;
            let spaces = match tok {
                Tok::Lit(s) => {
                    let trimmed = s.trim_start();
                    if trimmed.is_empty() && s.ends_with('\n') {
                        usize::MAX
                    } else {
                        adjust.push(i);
                        s.get(..s.len() - trimmed.len())
                            .map_or(0, |w| w.graphemes(true).count())
                    }
                }
                _ => 0,
            };
            min = min.min(spaces);
        }
        if let Tok::Lit(s) = tok {
            newline = s.ends_with('\n');
        }
    }
    for i in adjust {
        if let Some((Tok::Lit(s), _)) = toks.get_mut(i) {
            let cut: usize = s.graphemes(true).take(min).map(str::len).sum();
            *s = s.get(cut..).unwrap_or("").to_owned();
        }
    }
}

impl Evaluator<'_> {
    /// The template's parts, after Go's trimming (and dedent, for `flush`).
    pub(super) fn parts<'a>(
        &self,
        t: &'a Template,
        lits: &Literals<'_>,
        flush_heredoc: bool,
    ) -> Vec<Part<'a>> {
        let mut toks = Vec::new();
        flatten(t, lits, &mut toks);
        trim(&mut toks);
        if flush_heredoc {
            flush(&mut toks);
        }
        let mut it = toks.into_iter().map(|(t, _)| t).peekable();
        self.build(&mut it)
    }

    /// Rebuilds the nesting from the flat stream, stopping at the `else`/`endif`/`endfor` that closes
    /// the caller's directive (left in the stream for it).
    fn build<'a, I: Iterator<Item = Tok<'a>>>(
        &self,
        it: &mut std::iter::Peekable<I>,
    ) -> Vec<Part<'a>> {
        let mut parts = Vec::new();
        while let Some(tok) = it.next_if(|t| !matches!(t, Tok::Else | Tok::EndIf | Tok::EndFor)) {
            match tok {
                Tok::Lit(s) => parts.push(Part::Lit(s)),
                Tok::Interp(e) => parts.push(Part::Interp(e)),
                Tok::If(cond, span) => {
                    let then = self.build(it);
                    let otherwise = if it.next_if(|t| matches!(t, Tok::Else)).is_some() {
                        self.build(it)
                    } else {
                        Vec::new()
                    };
                    it.next_if(|t| matches!(t, Tok::EndIf));
                    parts.push(Part::If {
                        cond,
                        then,
                        otherwise,
                        at: self.span_range(span),
                    });
                }
                Tok::For(head, span) => {
                    let body = self.build(it);
                    it.next_if(|t| matches!(t, Tok::EndFor));
                    parts.push(Part::For {
                        head,
                        body,
                        at: self.span_range(span),
                    });
                }
                Tok::Else | Tok::EndIf | Tok::EndFor => {}
            }
        }
        parts
    }
}
