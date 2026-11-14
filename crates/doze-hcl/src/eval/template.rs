//! Templates: quoted strings with `${}` and `%{}`, and heredocs (hclsyntax's `TemplateExpr`,
//! `TemplateJoinExpr`, `TemplateWrapExpr`, and the parser's strip markers).
//!
//! The parts come from `tokens` (Go's token stream); here they are evaluated: `%{if}` as a
//! conditional between two templates, `%{for}` as a joined `for`.

use super::tokens::Literals;
use super::{Context, Evaluator, Out};
use crate::convert::convert;
use crate::diag::{Diagnostic, Diagnostics, Range};
use crate::types::Type;
use crate::value::Value;
use hcl_edit::Span as _;
use hcl_edit::expr::Expression;
use hcl_edit::template::{Element, HeredocTemplate, StringTemplate};

/// A template part, as Go's template parser builds them.
pub(super) enum Part<'a> {
    Lit(String),
    Interp(&'a Expression),
    If {
        cond: &'a Expression,
        then: Vec<Part<'a>>,
        otherwise: Vec<Part<'a>>,
        at: Range,
    },
    For {
        head: &'a hcl_edit::template::ForTemplateExpr,
        body: Vec<Part<'a>>,
        at: Range,
    },
}

impl<'s> Evaluator<'s> {
    pub(super) fn string_template(&self, t: &StringTemplate, ctx: &Context<'_>) -> Out {
        // A lone interpolation passes its value through untouched (Go's TemplateWrapExpr).
        if let [Element::Interpolation(i)] = t.iter().collect::<Vec<_>>().as_slice() {
            return self.eval(&i.expr, ctx);
        }
        let parts = self.parts(t, &Literals::Quoted, false);
        self.template_value(&parts, self.range(t), ctx)
    }

    pub(super) fn heredoc(&self, h: &HeredocTemplate, ctx: &Context<'_>) -> Out {
        if let [Element::Interpolation(i)] = h.template.iter().collect::<Vec<_>>().as_slice() {
            return self.eval(&i.expr, ctx);
        }
        let flush = h
            .span()
            .and_then(|s| self.src.text().get(s))
            .is_some_and(|raw| raw.starts_with("<<-"));
        let parts = self.parts(&h.template, &Literals::Heredoc(self.src.text()), flush);
        self.template_value(&parts, self.range(h), ctx)
    }

    fn part_range(&self, p: &Part<'_>) -> Range {
        match p {
            Part::Lit(_) => Range::default(),
            Part::Interp(e) => self.range(*e),
            Part::If { at, .. } | Part::For { at, .. } => *at,
        }
    }

    /// Go's `TemplateExpr.Value`: every part converted to a string and joined.
    pub(super) fn template_value(
        &self,
        parts: &[Part<'_>],
        whole: Range,
        ctx: &Context<'_>,
    ) -> Out {
        let mut buf = String::new();
        let mut diags = Diagnostics::default();
        let mut known = true;
        for part in parts {
            let (v, d) = self.part_value(part, ctx);
            diags.extend(d);
            let at = self.part_range(part);
            if v.is_null() {
                let d = Diagnostic::error(
                    "Invalid template interpolation value",
                    "The expression result is null. Cannot include a null value in a string template.",
                    at,
                );
                diags.push(d.with_context(whole));
                continue;
            }
            if !v.is_known() {
                known = false;
                continue;
            }
            match convert(&v, &Type::String) {
                Ok(Value::String(s)) if known && !diags.has_errors() => buf.push_str(&s),
                Ok(_) => {}
                Err(e) => {
                    let detail =
                        format!("Cannot include the given value in a string template: {e}.");
                    diags.push(
                        Diagnostic::error("Invalid template interpolation value", detail, at)
                            .with_context(whole),
                    );
                }
            }
        }
        if !known {
            return (Value::Unknown(Type::String), diags);
        }
        (Value::string(&buf), diags)
    }

    fn part_value(&self, part: &Part<'_>, ctx: &Context<'_>) -> Out {
        match part {
            Part::Lit(s) => (Value::string(s), Diagnostics::default()),
            Part::Interp(e) => self.eval(e, ctx),
            Part::If {
                cond,
                then,
                otherwise,
                at,
            } => self.template_if(cond, then, otherwise, *at, ctx),
            Part::For { head, body, at } => self.template_for(head, body, *at, ctx),
        }
    }

    /// `%{if}`: a conditional between two string templates.
    fn template_if(
        &self,
        cond: &Expression,
        then: &[Part<'_>],
        otherwise: &[Part<'_>],
        at: Range,
        ctx: &Context<'_>,
    ) -> Out {
        let (t, t_diags) = self.template_value(then, at, ctx);
        let (f, f_diags) = self.template_value(otherwise, at, ctx);
        let (c, mut diags) = self.eval(cond, ctx);
        let cond_at = self.range(cond);
        if c.is_null() {
            let d = Diagnostic::error(
                "Null condition",
                "The condition value is null. Conditions must either be true or false.",
                cond_at,
            );
            diags.push(d.with_context(at));
            return (Value::Unknown(Type::String), diags);
        }
        if !c.is_known() {
            return (Value::Unknown(Type::String), diags);
        }
        match convert(&c, &Type::Bool) {
            Ok(Value::Bool(true)) => {
                diags.extend(t_diags);
                (t, diags)
            }
            Ok(Value::Bool(false)) => {
                diags.extend(f_diags);
                (f, diags)
            }
            _ => {
                let d = Diagnostic::error(
                    "Incorrect condition type",
                    "The condition expression must be of type bool.",
                    cond_at,
                );
                diags.push(d.with_context(at));
                (Value::Unknown(Type::String), diags)
            }
        }
    }

    /// `%{for}`: a `for` producing one string per element, joined (Go's `TemplateJoinExpr`).
    fn template_for(
        &self,
        head: &hcl_edit::template::ForTemplateExpr,
        body: &[Part<'_>],
        at: Range,
        ctx: &Context<'_>,
    ) -> Out {
        let spec = super::for_expr::Spec {
            key_var: head.key_var.as_ref().map(|k| k.as_str()),
            value_var: head.value_var.as_str(),
            coll: &head.collection_expr,
            cond: None,
            whole: at,
        };
        let (tuple, mut diags) = self.for_tuple(
            &spec,
            &|c: &Context<'_>| self.template_value(body, at, c),
            ctx,
        );
        let Value::Tuple(items) = tuple else {
            return (Value::Unknown(Type::String), diags);
        };
        let mut buf = String::new();
        for v in items {
            if v.is_null() {
                let d = Diagnostic::error(
                    "Invalid template interpolation value",
                    "An iteration result is null. Cannot include a null value in a string template.",
                    at,
                );
                diags.push(d);
                continue;
            }
            if v.ty() == Type::Dynamic {
                return (Value::Unknown(Type::String), diags);
            }
            match convert(&v, &Type::String) {
                Err(e) => {
                    let detail = format!(
                        "Cannot include one of the interpolation results into the string template: {e}."
                    );
                    diags.push(Diagnostic::error(
                        "Invalid template interpolation value",
                        detail,
                        at,
                    ));
                }
                Ok(_) if !v.is_known() => return (Value::Unknown(Type::String), diags),
                Ok(Value::String(s)) => buf.push_str(&s),
                Ok(_) => {}
            }
        }
        (Value::string(&buf), diags)
    }
}
