//! Syntax Go's parser refuses and hcl-edit accepts, checked after parsing:
//!
//! - a chained legacy index, `a.1.0` (Go lexes `1.0` as one number: "Invalid legacy index syntax");
//! - an attribute splat inside another, `a.*.b.*` ("Nested splat expression not allowed");
//! - a number literal Go cannot read, such as an exponent beyond int32 ("Invalid number literal").

use super::Source;
use crate::diag::Diagnostic;
use hcl_edit::Span;
use hcl_edit::expr::{Traversal, TraversalOperator as Op};
use hcl_edit::structure::Body;
use hcl_edit::visit::{Visit, visit_traversal};

struct Rules<'s> {
    src: &'s Source,
    found: Option<Diagnostic>,
}

impl Visit for Rules<'_> {
    fn visit_number(&mut self, n: &hcl_edit::repr::Formatted<hcl_edit::Number>) {
        let text = n.as_repr().map(|r| r.to_string()).unwrap_or_default();
        let digits = text
            .find(|c: char| c.is_ascii_digit())
            .and_then(|i| text.get(i..))
            .unwrap_or("");
        if self.found.is_none() && crate::number::Number::parse(digits).is_none() {
            let at = n.span().map(|s| self.src.range(s)).unwrap_or_default();
            self.found = Some(Diagnostic::error(
                "Invalid number literal",
                "Failed to recognize the value of this number literal.",
                at,
            ));
        }
    }

    fn visit_traversal(&mut self, t: &Traversal) {
        if self.found.is_none() {
            self.found = self.check(t);
        }
        visit_traversal(self, t);
    }
}

impl Rules<'_> {
    fn check(&self, t: &Traversal) -> Option<Diagnostic> {
        let mut in_attr_splat = false;
        let mut prev_legacy_end: Option<(usize, usize)> = None;
        for op in &t.operators {
            let span = op.span()?;
            match op.value() {
                Op::LegacyIndex(_) => {
                    if let Some((start, _)) = prev_legacy_end.filter(|(_, end)| *end == span.start)
                    {
                        let at = self.src.range(start + 1..span.end);
                        return Some(Diagnostic::error(
                            "Invalid legacy index syntax",
                            "When using the legacy index syntax, chaining two indexes together is not permitted. Use the proper index syntax instead, like [1][0].",
                            at,
                        ));
                    }
                    prev_legacy_end = Some((span.start, span.end));
                    continue;
                }
                Op::AttrSplat(_) if in_attr_splat => {
                    let at = self.src.range(span.start + 1..span.end);
                    return Some(Diagnostic::error(
                        "Nested splat expression not allowed",
                        "A splat expression (*) cannot be used inside another attribute-only splat expression.",
                        at,
                    ));
                }
                Op::AttrSplat(_) => in_attr_splat = true,
                Op::GetAttr(_) => {}
                Op::Index(_) | Op::FullSplat(_) => in_attr_splat = false,
            }
            prev_legacy_end = None;
        }
        None
    }
}

/// The first of Go's extra syntax rules the body breaks, if any.
pub(super) fn check(src: &Source, body: &Body) -> Result<(), Diagnostic> {
    let mut rules = Rules { src, found: None };
    rules.visit_body(body);
    rules.found.map_or(Ok(()), Err)
}
