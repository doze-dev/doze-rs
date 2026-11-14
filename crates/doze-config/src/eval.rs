//! Expressions evaluated as doze evaluates them: hcl's diagnostics, in the expression's file, with
//! doze's better words for a missing attribute (Go's `attributeHint`).

use crate::error::nearest;
use crate::model::Kind;
use crate::refs::{Step, Traversal, variables};
use doze_hcl::Value;
use doze_hcl::decode::Attr;
use doze_hcl::diag::{Diagnostic, Diagnostics, Range};
use doze_hcl::eval::{Context, Evaluator};
use doze_hcl::syntax::Source;
use doze_hcl::types::Type;
use hcl_edit::expr::Expression;

/// An expression's value and diagnostics, in `file`.
pub(crate) fn eval_expr(
    expr: &Expression,
    src: &Source,
    file: &str,
    ctx: &Context<'_>,
) -> (Value, Diagnostics) {
    let (v, ds) = Evaluator::new(src).eval(expr, ctx);
    let ds = Diagnostics(ds.0.into_iter().map(|d| d.in_file(file)).collect());
    (v, hinted(ds, expr, src, ctx))
}

/// An attribute's value (`attr.Expr.Value(ctx)`).
pub(crate) fn eval(attr: &Attr<'_>, ctx: &Context<'_>) -> (Value, Diagnostics) {
    eval_expr(attr.expr, attr.src, attr.file, ctx)
}

/// gohcl's `DecodeExpression` into a Go value of type `ty` (see `doze_hcl::decode::value`).
pub(crate) fn decode(
    attr: &Attr<'_>,
    ctx: &Context<'_>,
    ty: &Type,
    nullable: bool,
) -> (Value, Diagnostics) {
    let (v, ds) = doze_hcl::decode::value(attr, ctx, ty, nullable);
    (v, hinted(ds, attr.expr, attr.src, ctx))
}

/// gohcl into a Go `string`.
pub(crate) fn decode_string(attr: &Attr<'_>, ctx: &Context<'_>) -> Result<String, Diagnostics> {
    let (v, ds) = decode(attr, ctx, &Type::String, false);
    if ds.has_errors() {
        return Err(ds);
    }
    Ok(v.as_str().unwrap_or_default().to_owned())
}

/// gohcl into a Go `bool`.
pub(crate) fn decode_bool(attr: &Attr<'_>, ctx: &Context<'_>) -> Result<bool, Diagnostics> {
    let (v, ds) = decode(attr, ctx, &Type::Bool, false);
    if ds.has_errors() {
        return Err(ds);
    }
    Ok(v.as_bool().unwrap_or(false))
}

/// gohcl into a Go `int` (64 bits): a whole number in range, or gocty's words.
pub(crate) fn decode_int(attr: &Attr<'_>, ctx: &Context<'_>) -> Result<i64, Diagnostics> {
    let (v, mut ds) = decode(attr, ctx, &Type::Number, false);
    if ds.has_errors() {
        return Err(ds);
    }
    match v.as_number().and_then(doze_hcl::Number::to_i64) {
        Some(n) => Ok(n),
        None => {
            let ev = Evaluator::new(attr.src);
            ds.push(
                Diagnostic::error(
                    "Unsuitable value type",
                    format!(
                        "Unsuitable value: value must be a whole number, between {} and {}",
                        i64::MIN,
                        i64::MAX
                    ),
                    ev.start_range(attr.expr),
                )
                .with_context(attr.range_of_expr())
                .in_file(attr.file),
            );
            Err(ds)
        }
    }
}

/// The range of an attribute's expression.
pub(crate) trait ExprRange {
    fn range_of_expr(&self) -> Range;
}

impl ExprRange for Attr<'_> {
    fn range_of_expr(&self) -> Range {
        use hcl_edit::Span;
        self.expr
            .span()
            .map(|s| self.src.range(s))
            .unwrap_or_default()
    }
}

/// Go's `addFixHints` for "Unsupported attribute": what the referenced object does have.
fn hinted(mut ds: Diagnostics, expr: &Expression, src: &Source, ctx: &Context<'_>) -> Diagnostics {
    if !ds.iter().any(|d| d.summary == "Unsupported attribute") {
        return ds;
    }
    let ts = variables(expr, src);
    for d in &mut ds.0 {
        if d.summary != "Unsupported attribute" {
            continue;
        }
        let Some(at) = d.subject else { continue };
        let found = ts
            .iter()
            .find(|t| t.range.start.byte <= at.start.byte && at.end.byte <= t.range.end.byte);
        if let Some(better) = found.and_then(|t| attribute_hint(t, ctx)) {
            d.detail = better;
        }
    }
    ds
}

fn attribute_hint(t: &Traversal, ctx: &Context<'_>) -> Option<String> {
    let mut v = ctx.get(&t.root)?.clone();
    let mut path = t.root.clone();
    for step in &t.steps {
        let Step::Attr(name) = step else {
            return None;
        };
        let names: Vec<String> = match &v {
            Value::Object(m) | Value::Map(_, m) => m.keys().cloned().collect(),
            _ => return None,
        };
        if !names.contains(name) {
            if path == t.root && Kind::parse(&path).is_some() {
                return Some(format!(
                    "nothing called {} is published as {path}.{name} here: a block cannot refer to itself, and a {path} must be declared to be referenced",
                    doze_hcl::value::quote::go_quote(name)
                ));
            }
            let mut msg = format!(
                "{path} has no attribute {}",
                doze_hcl::value::quote::go_quote(name)
            );
            if let Some(near) = nearest(name, &names) {
                msg.push_str(&format!(". Did you mean {path}.{near}?"));
            }
            return Some(format!("{msg} It has: {}.", names.join(", ")));
        }
        v = match &v {
            Value::Object(m) | Value::Map(_, m) => m.get(name).cloned()?,
            _ => return None,
        };
        path = format!("{path}.{name}");
    }
    None
}

/// `e.Value(nil)` is null with no errors (Go's `exprIsNull`).
pub(crate) fn expr_is_null(attr: &Attr<'_>) -> bool {
    let (v, ds) = Evaluator::new(attr.src).eval(attr.expr, &Context::default());
    !ds.has_errors() && v.is_null()
}
