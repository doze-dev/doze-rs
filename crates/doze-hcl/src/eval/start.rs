//! hclsyntax's `StartRange`: where a diagnostic about an expression as a whole begins. Function
//! argument diagnostics point there: inside the quotes of a string, at the left operand of a binary
//! operator, at the `[` of a dynamic index, at the marker of a splat.

use super::Evaluator;
use crate::diag::Range;
use hcl_edit::Span;
use hcl_edit::expr::{Expression, TraversalOperator};
use hcl_edit::template::{Directive, Element, Template};

impl Evaluator<'_> {
    pub(crate) fn start_range(&self, e: &Expression) -> Range {
        let whole = self.range(e);
        match e {
            Expression::String(s) => {
                let Some(span) = e.span() else { return whole };
                // The literal part starts after the quote; an empty one at the end of the string.
                if s.value().is_empty() {
                    return self.src.range(span.end..span.end);
                }
                self.src
                    .range(span.start + 1..span.end.saturating_sub(1).max(span.start + 1))
            }
            Expression::StringTemplate(t) => {
                let start = e.span().map_or(0, |s| s.start + 1);
                self.template_start(t, start).unwrap_or(whole)
            }
            Expression::HeredocTemplate(h) => {
                let start = e
                    .span()
                    .and_then(|s| {
                        let text = self.src.text().get(s.clone())?;
                        text.find('\n').map(|i| s.start + i + 1)
                    })
                    .unwrap_or(0);
                self.template_start(&h.template, start).unwrap_or(whole)
            }
            Expression::Parenthesis(p) => self.start_range(p.inner()),
            Expression::BinaryOp(b) => self.start_range(&b.lhs_expr),
            Expression::Conditional(c) => self.start_range(&c.cond_expr),
            Expression::Traversal(t) => self.traversal_start(t, whole),
            // A negative literal is a unary minus in Go: its `-`.
            Expression::Number(_)
                if self
                    .src
                    .text()
                    .get(whole_span(e))
                    .is_some_and(|t| t.starts_with('-')) =>
            {
                let at = e.span().map_or(0, |s| s.start);
                self.src.range(at..at + 1)
            }
            // An opening bracket, a unary operator: the first token.
            Expression::Array(_)
            | Expression::Object(_)
            | Expression::ForExpr(_)
            | Expression::UnaryOp(_) => {
                let at = e.span().map_or(0, |s| s.start);
                self.src.range(at..at + 1)
            }
            // The name and its opening parenthesis.
            Expression::FuncCall(_) => {
                let Some(span) = e.span() else { return whole };
                let open = self
                    .src
                    .text()
                    .get(span.clone())
                    .and_then(|t| t.find('('))
                    .map_or(span.end, |i| span.start + i + 1);
                self.src.range(span.start..open)
            }
            _ => whole,
        }
    }

    fn traversal_start(&self, t: &hcl_edit::expr::Traversal, whole: Range) -> Range {
        let first_splat = t.operators.iter().find(|op| {
            matches!(
                op.value(),
                TraversalOperator::AttrSplat(_) | TraversalOperator::FullSplat(_)
            )
        });
        let dynamic_last = t
            .operators
            .last()
            .filter(|op| matches!(op.value(), TraversalOperator::Index(k) if !is_literal(k)));
        match (first_splat, dynamic_last) {
            (Some(op), _) => self.range(op),
            // An index expression starts at its `[`.
            (None, Some(op)) => {
                let at = op.span().map_or(0, |s| s.start);
                self.src.range(at..at + 1)
            }
            (None, None) => whole,
        }
    }

    /// The start of a template's first part: a lone interpolation is the whole template (Go's
    /// `TemplateWrapExpr`); otherwise the first part's own start.
    fn template_start(&self, t: &Template, literal_at: usize) -> Option<Range> {
        let elems: Vec<&Element> = t.iter().collect();
        match elems.as_slice() {
            [Element::Interpolation(_)] => None,
            [Element::Interpolation(i), ..] => Some(self.start_range(&i.expr)),
            [Element::Directive(d), ..] => match d.as_ref() {
                Directive::If(i) => Some(self.start_range(&i.if_expr.cond_expr)),
                // The `%{for …}` opener.
                Directive::For(_) => {
                    let close = self
                        .src
                        .text()
                        .get(literal_at..)
                        .and_then(|t| t.find('}'))
                        .map_or(literal_at, |i| literal_at + i + 1);
                    Some(self.src.range(literal_at..close))
                }
            },
            // The first literal part, as Go's scanner splits it (up to a sequence or the end).
            [Element::Literal(l), ..] => {
                let end = l.span().map_or(literal_at, |s| s.end);
                Some(self.src.range(literal_at..end.max(literal_at)))
            }
            _ => Some(self.src.range(literal_at..literal_at)),
        }
    }
}

/// An index key Go's parser folds into a static traversal step: a literal or a quoted string.
fn is_literal(k: &Expression) -> bool {
    matches!(
        k,
        Expression::Number(_) | Expression::Bool(_) | Expression::Null(_) | Expression::String(_)
    )
}

fn whole_span(e: &Expression) -> std::ops::Range<usize> {
    e.span().unwrap_or_default()
}
