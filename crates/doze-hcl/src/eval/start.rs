//! hclsyntax's `StartRange`: where a diagnostic about an expression as a whole begins. Function
//! argument diagnostics point there: inside the quotes of a string, at the left operand of a binary
//! operator, at the `[` of a dynamic index, at the marker of a splat.

use super::Evaluator;
use crate::diag::Range;
use hcl_edit::Span;
use hcl_edit::expr::{Expression, TraversalOperator};
use hcl_edit::template::{Directive, Element, Template};

impl Evaluator<'_> {
    pub(super) fn start_range(&self, e: &Expression) -> Range {
        let whole = self.range(e);
        match e {
            Expression::String(s) => {
                let Some(span) = e.span() else { return whole };
                // The literal part starts after the quote; an empty one at the end of the string.
                let at = if s.value().is_empty() {
                    span.end
                } else {
                    span.start + 1
                };
                self.src.range(at..at)
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
            Expression::Traversal(t) => {
                let first_splat = t.operators.iter().find(|op| {
                    matches!(
                        op.value(),
                        TraversalOperator::AttrSplat(_) | TraversalOperator::FullSplat(_)
                    )
                });
                let dynamic_last = t.operators.last().filter(
                    |op| matches!(op.value(), TraversalOperator::Index(k) if !is_literal(k)),
                );
                match first_splat.or(dynamic_last) {
                    Some(op) => self.range(op),
                    None => whole,
                }
            }
            _ => whole,
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
                Directive::For(_) => Some(self.src.range(literal_at..literal_at)),
            },
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
