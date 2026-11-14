//! Splats: `a[*].b` and `a.*.b` (hclsyntax's `SplatExpr`).

use super::{Context, Evaluator, Out};
use crate::diag::{Diagnostic, Diagnostics, Range};
use crate::types::Type;
use crate::value::Value;
use hcl_edit::expr::TraversalOperator as Op;
use hcl_edit::repr::Decorated;

impl Evaluator<'_> {
    /// Go's `SplatExpr.Value`: `each` applied to every element of a list, set or tuple; any other
    /// value is first wrapped in a one-element tuple (a null in an empty one).
    pub(super) fn splat(
        &self,
        source: Value,
        diags: Diagnostics,
        marker: Range,
        each: &[Decorated<Op>],
        ctx: &Context<'_>,
    ) -> Out {
        if diags.has_errors() {
            return (Value::DYNAMIC, diags);
        }
        let ty = source.ty();
        let sequence = matches!(ty, Type::List(_) | Type::Set(_) | Type::Tuple(_));
        if source.is_null() {
            if !sequence {
                return (Value::empty_tuple(), diags);
            }
            let mut diags = diags;
            let d = Diagnostic::error(
                "Splat of null value",
                "Splat expressions (with the * symbol) cannot be applied to null sequences.",
                marker,
            );
            diags.push(d);
            return (Value::DYNAMIC, diags);
        }
        if ty == Type::Dynamic {
            return (Value::DYNAMIC, diags);
        }
        let (source, upgraded_unknown) = if sequence {
            (source, false)
        } else {
            (Value::Tuple(vec![source.clone()]), !source.is_known())
        };
        let ty = source.ty();
        if !source.is_known() {
            let (rt, d) = self.splat_type(&ty, each, ctx);
            let mut diags = diags;
            diags.extend(d);
            return (Value::Unknown(rt), diags);
        }
        let mut diags = diags;
        let mut vals = Vec::new();
        let mut known = true;
        for (_, item) in source.elements() {
            let (v, d) = self.walk(item, Diagnostics::default(), marker, each, ctx);
            if d.has_errors() {
                known = false;
            }
            diags.extend(d);
            vals.push(v);
        }
        if upgraded_unknown {
            return (Value::DYNAMIC, diags);
        }
        if !known {
            return (Value::Unknown(self.splat_type(&ty, each, ctx).0), diags);
        }
        if vals.is_empty() && matches!(ty, Type::List(_) | Type::Set(_)) {
            let rt = self.splat_type(&ty, each, ctx).0;
            return (
                Value::List(rt.element().cloned().unwrap_or(Type::Dynamic), Vec::new()),
                diags,
            );
        }
        splat_result(&ty, vals, diags, marker)
    }
}

/// The splat's value from its elements: a list for a list or set, a tuple for a tuple.
fn splat_result(ty: &Type, vals: Vec<Value>, mut diags: Diagnostics, marker: Range) -> Out {
    {
        match ty {
            Type::List(_) | Type::Set(_) => match Value::list(Type::Dynamic, vals) {
                Ok(v) => (v, diags),
                Err(_) => {
                    let d = Diagnostic::error(
                        "Invalid nested splat expressions",
                        "The second level of splat expression produced elements of different types, so it isn't possible to construct a valid list to represent the top-level result.\n\nConsider using a for expression instead, to produce a tuple-typed result which can therefore have non-homogenous element types.",
                        marker,
                    );
                    diags.push(d);
                    (Value::DYNAMIC, diags)
                }
            },
            _ => (Value::Tuple(vals), diags),
        }
    }
}

impl Evaluator<'_> {
    /// The result type of a splat over a value of type `ty`, found by applying `each` to unknowns of
    /// the element types.
    fn splat_type(
        &self,
        ty: &Type,
        each: &[Decorated<Op>],
        ctx: &Context<'_>,
    ) -> (Type, Diagnostics) {
        let mut diags = Diagnostics::default();
        let mut each_ty = |ety: &Type| {
            let (v, d) = self.walk(
                Value::Unknown(ety.clone()),
                Diagnostics::default(),
                Range::default(),
                each,
                ctx,
            );
            diags.extend(d);
            v.ty()
        };
        let t = match ty {
            Type::List(e) | Type::Set(e) => Type::list(each_ty(e)),
            Type::Tuple(es) => Type::Tuple(es.iter().map(&mut each_ty).collect()),
            _ => Type::Dynamic,
        };
        (t, diags)
    }
}
