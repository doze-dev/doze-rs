//! `[for k, v in coll : value if cond]` and `{for k, v in coll : key => value... if cond}` (hclsyntax's
//! `ForExpr`). Also the engine of a template's `%{for}`.

use super::for_cond::Keep;
use super::{Context, Evaluator, Out};
use crate::convert::convert;
use crate::diag::{Diagnostic, Diagnostics, Range};
use crate::types::Type;
use crate::value::Value;
use hcl_edit::expr::{Expression, ForExpr};
use std::collections::BTreeMap;

pub(super) struct Spec<'a> {
    pub(super) key_var: Option<&'a str>,
    pub(super) value_var: &'a str,
    pub(super) coll: &'a Expression,
    pub(super) cond: Option<&'a Expression>,
    pub(super) whole: Range,
}

type ValueFn<'f> = dyn Fn(&Context<'_>) -> Out + 'f;

impl Evaluator<'_> {
    pub(super) fn for_expr(&self, f: &ForExpr, ctx: &Context<'_>) -> Out {
        let spec = Spec {
            key_var: f.intro.key_var.as_ref().map(|k| k.as_str()),
            value_var: f.intro.value_var.as_str(),
            coll: &f.intro.collection_expr,
            cond: f.cond.as_ref().map(|c| &c.expr),
            whole: self.range(f),
        };
        let value = |c: &Context<'_>| self.eval(&f.value_expr, c);
        match &f.key_expr {
            None => self.for_tuple(&spec, &value, ctx),
            Some(key) => self.for_object(&spec, key, &value, f.grouping, ctx),
        }
    }

    /// The collection, checked; `Err` is the early result.
    fn for_collection(
        &self,
        spec: &Spec<'_>,
        ctx: &Context<'_>,
    ) -> Result<(Value, Diagnostics), Out> {
        let (coll, mut diags) = self.eval(spec.coll, ctx);
        let at = self.range(spec.coll);
        if coll.is_null() {
            let d = Diagnostic::error(
                "Iteration over null value",
                "A null value cannot be used as the collection in a 'for' expression.",
                at,
            );
            diags.push(d.with_context(spec.whole));
            return Err((Value::DYNAMIC, diags));
        }
        if coll.ty() == Type::Dynamic {
            return Err((Value::DYNAMIC, diags));
        }
        if !coll.can_iterate() {
            let detail = format!(
                "A value of type {} cannot be used as the collection in a 'for' expression.",
                coll.ty().friendly_name()
            );
            diags.push(
                Diagnostic::error("Iteration over non-iterable value", detail, at)
                    .with_context(spec.whole),
            );
            return Err((Value::DYNAMIC, diags));
        }
        if let Some(cond) = spec.cond {
            // Go checks the condition once with dynamic unknowns for the variables, before iterating.
            let child = ctx.child(self.vars(spec, Value::DYNAMIC, Value::DYNAMIC));
            let (r, d) = self.eval(cond, &child);
            let failed = d.has_errors();
            diags.extend(d);
            let cat = self.range(cond);
            if r.is_null() {
                let d = Diagnostic::error(
                    "Condition is null",
                    "The value of the 'if' clause must not be null.",
                    cat,
                );
                diags.push(d.with_context(spec.whole));
                return Err((Value::DYNAMIC, diags));
            }
            if let Err(e) = convert(&r, &Type::Bool) {
                let d = Diagnostic::error(
                    "Invalid 'for' condition",
                    format!("The 'if' clause value is invalid: {e}."),
                    cat,
                );
                diags.push(d.with_context(spec.whole));
                return Err((Value::DYNAMIC, diags));
            }
            if failed {
                return Err((Value::DYNAMIC, diags));
            }
        }
        if !coll.is_known() {
            return Err((Value::DYNAMIC, diags));
        }
        Ok((coll, diags))
    }

    fn vars(&self, spec: &Spec<'_>, k: Value, v: Value) -> BTreeMap<String, Value> {
        let mut vars = BTreeMap::new();
        if let Some(kv) = spec.key_var {
            vars.insert(kv.to_owned(), k);
        }
        vars.insert(spec.value_var.to_owned(), v);
        vars
    }

    pub(super) fn for_tuple(&self, spec: &Spec<'_>, value: &ValueFn<'_>, ctx: &Context<'_>) -> Out {
        let (coll, mut diags) = match self.for_collection(spec, ctx) {
            Ok(c) => c,
            Err(out) => return out,
        };
        let mut vals = Vec::new();
        let mut known = true;
        for (k, v) in coll.elements() {
            let child = ctx.child(self.vars(spec, k, v));
            match self.keep(spec, &child, known, true, &mut diags) {
                Keep::No => continue,
                Keep::Unknown => {
                    known = false;
                    continue;
                }
                Keep::Yes => {}
            }
            let (val, d) = value(&child);
            diags.extend(d);
            vals.push(val);
        }
        if !known {
            return (Value::DYNAMIC, diags);
        }
        (Value::Tuple(vals), diags)
    }

    fn for_object(
        &self,
        spec: &Spec<'_>,
        key: &Expression,
        value: &ValueFn<'_>,
        group: bool,
        ctx: &Context<'_>,
    ) -> Out {
        let (coll, mut diags) = match self.for_collection(spec, ctx) {
            Ok(c) => c,
            Err(out) => return out,
        };
        let mut single: BTreeMap<String, Value> = BTreeMap::new();
        let mut grouped: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        let mut known = true;
        let key_at = self.range(key);
        for (k, v) in coll.elements() {
            let child = ctx.child(self.vars(spec, k, v));
            match self.keep(spec, &child, known, false, &mut diags) {
                Keep::No => continue,
                Keep::Unknown => {
                    known = false;
                    continue;
                }
                Keep::Yes => {}
            }
            let (raw, d) = self.eval(key, &child);
            diags.extend(d);
            let name = match self.object_key_of(&raw, known, key_at, spec.whole, &mut diags) {
                Some(n) => n,
                None => {
                    known = false;
                    continue;
                }
            };
            let (val, d) = value(&child);
            diags.extend(d);
            if group {
                grouped.entry(name).or_default().push(val);
                continue;
            }
            if let std::collections::btree_map::Entry::Vacant(slot) = single.entry(name.clone()) {
                slot.insert(val);
            } else {
                let detail = format!(
                    "Two different items produced the key {} in this 'for' expression. If duplicates are expected, use the ellipsis (...) after the value expression to enable grouping by key.",
                    crate::value::quote::go_quote(&name)
                );
                diags.push(
                    Diagnostic::error("Duplicate object key", detail, key_at)
                        .with_context(spec.whole),
                );
            }
        }
        if !known {
            return (Value::DYNAMIC, diags);
        }
        if group {
            single = grouped
                .into_iter()
                .map(|(k, vs)| (k, Value::Tuple(vs)))
                .collect();
        }
        (Value::Object(single), diags)
    }

    /// The key a `for` object element gets; `None` (with an error the first time) when it has none.
    fn object_key_of(
        &self,
        raw: &Value,
        first: bool,
        at: Range,
        whole: Range,
        diags: &mut Diagnostics,
    ) -> Option<String> {
        if raw.is_null() {
            if first {
                let d = Diagnostic::error(
                    "Invalid object key",
                    "Key expression in 'for' expression must not produce a null value.",
                    at,
                );
                diags.push(d.with_context(whole));
            }
            return None;
        }
        if !raw.is_known() {
            return None;
        }
        match convert(raw, &Type::String) {
            Ok(Value::String(s)) => Some(s),
            Ok(_) => None,
            Err(e) => {
                if first {
                    let d = Diagnostic::error(
                        "Invalid object key",
                        format!("The key expression produced an invalid result: {e}."),
                        at,
                    );
                    diags.push(d.with_context(whole));
                }
                None
            }
        }
    }
}
