//! `cond ? a : b` (hclsyntax's `ConditionalExpr`): both results are evaluated and unified to one type
//! before the condition picks.

use super::{Context, Evaluator, Out};
use crate::convert::{Unified, apply, convert, unify};
use crate::diag::{Diagnostic, Range};
use crate::types::Type;
use crate::value::Value;
use hcl_edit::expr::Conditional;

/// How one result reaches the result type.
#[derive(Clone)]
enum Conv {
    Keep,
    To(Type),
    Dynamic,
}

impl Evaluator<'_> {
    pub(super) fn conditional(&self, c: &Conditional, ctx: &Context<'_>) -> Out {
        let whole = self.range(c);
        let (t, t_diags) = self.eval(&c.true_expr, ctx);
        let (f, f_diags) = self.eval(&c.false_expr, ctx);
        let Some((result_ty, convs)) = result_type(&t, &f) else {
            let at = Range::between(self.range(&c.true_expr), self.range(&c.false_expr));
            let detail = format!(
                "The true and false result expressions must have consistent types. {}.",
                mismatch_detail(&t.ty(), &f.ty())
            );
            let d = Diagnostic::error("Inconsistent conditional result types", detail, at)
                .with_context(whole);
            return (Value::DYNAMIC, d.into());
        };
        let (cond, mut diags) = self.eval(&c.cond_expr, ctx);
        let cond_at = self.range(&c.cond_expr);
        if cond.is_null() {
            let d = Diagnostic::error(
                "Null condition",
                "The condition value is null. Conditions must either be true or false.",
                cond_at,
            );
            diags.push(d.with_context(whole));
            return (Value::Unknown(result_ty), diags);
        }
        if !cond.is_known() {
            if t.is_null() && f.is_null() {
                return (Value::Null(result_ty), diags);
            }
            return (Value::Unknown(result_ty), diags);
        }
        let Ok(Value::Bool(pick)) = convert(&cond, &Type::Bool) else {
            let d = Diagnostic::error(
                "Incorrect condition type",
                "The condition expression must be of type bool.",
                cond_at,
            );
            diags.push(d.with_context(whole));
            return (Value::Unknown(result_ty), diags);
        };
        let (chosen, chosen_diags, conv, expr, which) = if pick {
            (t, t_diags, &convs[0], &c.true_expr, "true")
        } else {
            (f, f_diags, &convs[1], &c.false_expr, "false")
        };
        diags.extend(chosen_diags);
        let converted = match conv {
            Conv::Keep => Ok(chosen),
            Conv::Dynamic => Ok(Value::DYNAMIC),
            Conv::To(ty) => apply(&chosen, ty, true),
        };
        match converted {
            Ok(v) => (v, diags),
            Err(e) => {
                let detail = format!("The {which} result value has the wrong type: {e}.");
                let d = Diagnostic::error(
                    "Inconsistent conditional result types",
                    detail,
                    self.range(expr),
                );
                diags.push(d.with_context(whole));
                (Value::Unknown(result_ty), diags)
            }
        }
    }
}

/// hclsyntax's `describeConditionalTypeMismatch`, in short.
fn mismatch_detail(t: &Type, f: &Type) -> String {
    format!(
        "The 'true' value is {}, but the 'false' value is {}",
        t.friendly_name(),
        f.friendly_name()
    )
}

/// The result type and how each result reaches it; `None` when the types do not unify.
fn result_type(t: &Value, f: &Value) -> Option<(Type, [Conv; 2])> {
    let null_dynamic = Value::Null(Type::Dynamic);
    if *t == null_dynamic {
        return Some((f.ty(), [Conv::To(f.ty()), Conv::Keep]));
    }
    if *f == null_dynamic {
        return Some((t.ty(), [Conv::Keep, Conv::To(t.ty())]));
    }
    if t.ty() == Type::Dynamic || f.ty() == Type::Dynamic {
        return Some((Type::Dynamic, [Conv::Keep, Conv::Keep]));
    }
    match unify(&[t.ty(), f.ty()], true) {
        Unified::To(u) => Some((u.clone(), [Conv::To(u.clone()), Conv::To(u)])),
        Unified::AllDynamic => Some((Type::Dynamic, [Conv::Dynamic, Conv::Dynamic])),
        Unified::None => None,
    }
}
