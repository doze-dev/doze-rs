//! `[a, b]` and `{k = v}` (hclsyntax's `TupleConsExpr`, `ObjectConsExpr` and `ObjectConsKeyExpr`).

use super::{Context, Evaluator, Out};
use crate::convert::convert;
use crate::diag::{Diagnostic, Diagnostics};
use crate::types::Type;
use crate::value::Value;
use hcl_edit::expr::{Array, Expression, Object, ObjectKey};
use std::collections::BTreeMap;

impl Evaluator<'_> {
    pub(super) fn array(&self, a: &Array, ctx: &Context<'_>) -> Out {
        let mut diags = Diagnostics::default();
        let mut vals = Vec::with_capacity(a.len());
        for e in a.iter() {
            let (v, d) = self.eval(e, ctx);
            diags.extend(d);
            vals.push(v);
        }
        (Value::Tuple(vals), diags)
    }

    pub(super) fn object(&self, o: &Object, ctx: &Context<'_>) -> Out {
        let mut diags = Diagnostics::default();
        let mut vals = BTreeMap::new();
        let mut known = true;
        for (key, value) in o.iter() {
            let (k, kd) = self.object_key(key, ctx);
            let key_failed = kd.has_errors();
            diags.extend(kd);
            let (v, vd) = self.eval(value.expr(), ctx);
            diags.extend(vd);
            if key_failed {
                known = false;
                continue;
            }
            if k.is_null() {
                let d = Diagnostic::error(
                    "Null value as key",
                    "Can't use a null value as a key.",
                    self.range(value.expr()),
                );
                diags.push(d);
                known = false;
                continue;
            }
            let k = match convert(&k, &Type::String) {
                Ok(k) => k,
                Err(e) => {
                    let d = Diagnostic::error(
                        "Incorrect key type",
                        format!("Can't use this value as a key: {e}."),
                        self.key_range(key),
                    );
                    diags.push(d);
                    known = false;
                    continue;
                }
            };
            match k {
                Value::String(s) => {
                    vals.insert(s, v);
                }
                _ => known = false,
            }
        }
        if !known {
            return (Value::DYNAMIC, diags);
        }
        (Value::Object(vals), diags)
    }

    fn key_range(&self, key: &ObjectKey) -> crate::diag::Range {
        match key {
            ObjectKey::Ident(i) => self.range(i),
            ObjectKey::Expression(e) => self.range(e),
        }
    }

    /// A bare name is a literal key; a dotted name is refused as ambiguous (it reads as a reference
    /// but is not one); anything else is evaluated.
    fn object_key(&self, key: &ObjectKey, ctx: &Context<'_>) -> Out {
        match key {
            ObjectKey::Ident(i) => (Value::string(i.as_str()), Diagnostics::default()),
            ObjectKey::Expression(Expression::Traversal(t)) if is_dotted_name(t) => {
                let d = Diagnostic::error(
                    "Ambiguous attribute key",
                    "If this expression is intended to be a reference, wrap it in parentheses. If it's instead intended as a literal name containing periods, wrap it in quotes to create a string literal.",
                    self.range(key_expr(key)),
                );
                (Value::DYNAMIC, d.into())
            }
            ObjectKey::Expression(Expression::Variable(v)) => {
                (Value::string(v.as_str()), Diagnostics::default())
            }
            // A bare keyword is a name here, as in Go: `{null = 1}` has the key "null".
            ObjectKey::Expression(Expression::Null(_)) => {
                (Value::string("null"), Diagnostics::default())
            }
            ObjectKey::Expression(Expression::Bool(b)) => (
                Value::string(if *b.value() { "true" } else { "false" }),
                Diagnostics::default(),
            ),
            ObjectKey::Expression(e) => self.eval(e, ctx),
        }
    }
}

fn key_expr(key: &ObjectKey) -> &dyn hcl_edit::Span {
    match key {
        ObjectKey::Ident(i) => i,
        ObjectKey::Expression(e) => e,
    }
}

/// `a.b` as Go's parser sees an object key: a traversal from a name through attribute names (or
/// legacy `.0` indexes) only.
fn is_dotted_name(t: &hcl_edit::expr::Traversal) -> bool {
    use hcl_edit::expr::TraversalOperator as Op;
    matches!(t.expr, Expression::Variable(_))
        && !t.operators.is_empty()
        && t.operators.iter().all(|o| {
            matches!(
                o.value(),
                Op::GetAttr(_) | Op::LegacyIndex(_) | Op::Index(_)
            )
        })
}
