//! `a.b`, `a[k]`, `a.0`, and the splats `a[*].b` and `a.*.b` (hcl's `Index` and `GetAttr`,
//! hclsyntax's `SplatExpr`).
//!
//! hcl-edit gives a flat list of operators. Go's parser makes a full splat (`[*]`) swallow every
//! operator after it, and an attribute splat (`.*`) only the attribute steps right after it; the
//! walk below does the same.

use super::{Context, Evaluator, Out};
use crate::convert::convert;
use crate::diag::{Diagnostic, Diagnostics, Range};
use crate::number::Number;
use crate::types::Type;
use crate::value::Value;
use crate::value::quote::go_quote;
use hcl_edit::expr::{Traversal, TraversalOperator as Op};
use hcl_edit::repr::Decorated;

impl Evaluator<'_> {
    pub(super) fn traversal(&self, t: &Traversal, ctx: &Context<'_>) -> Out {
        let (base, diags) = self.eval(&t.expr, ctx);
        self.walk(base, diags, self.range(&t.expr), &t.operators, ctx)
    }

    /// Applies `ops` to `val` (which came from an expression starting at `from`).
    pub(super) fn walk(
        &self,
        mut val: Value,
        mut diags: Diagnostics,
        from: Range,
        ops: &[Decorated<Op>],
        ctx: &Context<'_>,
    ) -> Out {
        let mut i = 0;
        while let Some(op) = ops.get(i) {
            let at = self.range(op);
            match op.value() {
                Op::GetAttr(name) => {
                    let (v, d) = get_attr(&val, name.as_str(), at);
                    val = v;
                    diags.extend(d);
                }
                Op::LegacyIndex(n) => {
                    let (v, d) = index(&val, &Value::Number(Number::from_u64(*n.value())), at);
                    val = v;
                    diags.extend(d);
                }
                Op::Index(e) => {
                    let (key, kd) = self.eval(e, ctx);
                    diags.extend(kd);
                    let (v, d) = index(&val, &key, at);
                    val = v;
                    diags.extend(d);
                }
                Op::FullSplat(_) => {
                    let rest = ops.get(i + 1..).unwrap_or(&[]);
                    return self.splat(val, diags, Range::between(from, at), rest, ctx);
                }
                Op::AttrSplat(_) => {
                    let n = ops
                        .get(i + 1..)
                        .unwrap_or(&[])
                        .iter()
                        .take_while(|o| matches!(o.value(), Op::GetAttr(_) | Op::LegacyIndex(_)))
                        .count();
                    let each = ops.get(i + 1..i + 1 + n).unwrap_or(&[]);
                    let (v, d) = self.splat(val, diags, Range::between(from, at), each, ctx);
                    val = v;
                    diags = d;
                    i += n;
                }
            }
            i += 1;
        }
        (val, diags)
    }
}

fn err(summary: &str, detail: impl Into<String>, at: Range) -> Out {
    (
        Value::DYNAMIC,
        Diagnostic::error(summary, detail, at).into(),
    )
}

fn ok(v: Value) -> Out {
    (v, Diagnostics::default())
}

/// hcl's `Index`.
pub(crate) fn index(coll: &Value, key: &Value, at: Range) -> Out {
    const INVALID: &str = "Invalid index";
    if coll.is_null() {
        return err(
            "Attempt to index null value",
            "This value is null, so it does not have any indices.",
            at,
        );
    }
    if key.is_null() {
        return err(INVALID, "Can't use a null value as an indexing key.", at);
    }
    let ty = coll.ty();
    if key.ty() == Type::Dynamic || ty == Type::Dynamic {
        return ok(Value::DYNAMIC);
    }
    match &ty {
        Type::List(_) | Type::Tuple(_) | Type::Map(_) => index_sequence_or_map(coll, &ty, key, at),
        Type::Object(o) => {
            let was_number = key.ty() == Type::Number;
            let key = match convert(key, &Type::String) {
                Ok(k) => k,
                Err(e) => {
                    return err(
                        INVALID,
                        format!(
                            "The given key does not identify an element in this collection value: {e}."
                        ),
                        at,
                    );
                }
            };
            let Value::String(name) = key else {
                return ok(Value::DYNAMIC);
            };
            let Some(aty) = o.attrs.get(&name) else {
                let hint = if was_number {
                    " An object only supports looking up attributes by name, not by numeric index."
                } else {
                    ""
                };
                return err(
                    INVALID,
                    format!(
                        "The given key does not identify an element in this collection value.{hint}"
                    ),
                    at,
                );
            };
            match coll {
                Value::Object(m) => ok(m
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| Value::Unknown(aty.clone()))),
                _ => ok(Value::Unknown(aty.clone())),
            }
        }
        Type::Set(_) => err(
            INVALID,
            "Elements of a set are identified only by their value and don't have any separate index or key to select with, so it's only possible to perform operations across all elements of the set.",
            at,
        ),
        _ => err(INVALID, "This value does not have any indices.", at),
    }
}

fn index_sequence_or_map(coll: &Value, ty: &Type, key: &Value, at: Range) -> Out {
    let want = if matches!(ty, Type::Map(_)) {
        Type::String
    } else {
        Type::Number
    };
    let key = match convert(key, &want) {
        Ok(k) => k,
        Err(e) => {
            return err(
                "Invalid index",
                format!(
                    "The given key does not identify an element in this collection value: {e}."
                ),
                at,
            );
        }
    };
    let unknown_result = || {
        if let Type::Tuple(_) = ty {
            Value::DYNAMIC
        } else {
            Value::Unknown(ty.element().cloned().unwrap_or(Type::Dynamic))
        }
    };
    if !coll.is_known() || !key.is_known() {
        return ok(unknown_result());
    }
    let found = match (coll, &key) {
        (Value::List(_, es) | Value::Tuple(es), Value::Number(n)) => n
            .to_i64()
            .and_then(|i| usize::try_from(i).ok())
            .and_then(|i| es.get(i))
            .cloned(),
        (Value::Map(_, m), Value::String(s)) => m.get(s).cloned(),
        _ => None,
    };
    if let Some(v) = found {
        return ok(v);
    }
    // Go explains a whole, non-negative index only when it equals the length (its check is
    // `index - len < 1` after the element was not found); larger ones get the plain message.
    let len = coll.len().map(Number::from_usize);
    let at_len = |n: &Number| {
        len.as_ref()
            .is_some_and(|l| n.cmp_value(l) == std::cmp::Ordering::Equal)
    };
    let detail = match (&key, coll.len()) {
        (Value::Number(n), _) if !n.is_integer() => {
            ": indexing a sequence requires a whole number, but the given index has a fractional part"
        }
        (Value::Number(n), _) if n.signum() < 0 => {
            ": a negative number is not a valid index for a sequence"
        }
        (Value::Number(n), Some(0)) if at_len(n) => ": the collection has no elements",
        (Value::Number(n), Some(_)) if at_len(n) => {
            ": the given index is greater than or equal to the length of the collection"
        }
        _ => "",
    };
    err(
        "Invalid index",
        format!("The given key does not identify an element in this collection value{detail}."),
        at,
    )
}

/// hcl's `GetAttr`.
pub(crate) fn get_attr(obj: &Value, name: &str, at: Range) -> Out {
    const UNSUPPORTED: &str = "Unsupported attribute";
    if obj.is_null() {
        return err(
            "Attempt to get attribute from null value",
            "This value is null, so it does not have any attributes.",
            at,
        );
    }
    let ty = obj.ty();
    match &ty {
        Type::Object(o) => match o.attrs.get(name) {
            None => err(
                UNSUPPORTED,
                format!(
                    "This object does not have an attribute named {}.",
                    go_quote(name)
                ),
                at,
            ),
            Some(aty) => match obj {
                Value::Object(m) => ok(m
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| Value::Unknown(aty.clone()))),
                _ => ok(Value::Unknown(aty.clone())),
            },
        },
        Type::Map(e) => match obj {
            Value::Map(_, m) => match m.get(name) {
                Some(v) => ok(v.clone()),
                None => err(
                    "Missing map element",
                    format!(
                        "This map does not have an element with the key {}.",
                        go_quote(name)
                    ),
                    at,
                ),
            },
            _ => ok(Value::Unknown((**e).clone())),
        },
        Type::Dynamic => ok(Value::DYNAMIC),
        Type::List(e) if matches!(**e, Type::Object(_)) => {
            let has = matches!(&**e, Type::Object(o) if o.attrs.contains_key(name));
            let detail = if has {
                format!(
                    "Can't access attributes on a list of objects. Did you mean to access attribute {} for a specific element of the list, or across all elements of the list?",
                    go_quote(name)
                )
            } else {
                "Can't access attributes on a list of objects. Did you mean to access an attribute for a specific element of the list, or across all elements of the list?".to_owned()
            };
            err(UNSUPPORTED, detail, at)
        }
        Type::Set(e) if matches!(**e, Type::Object(_)) => err(
            UNSUPPORTED,
            "Can't access attributes on a set of objects. Did you mean to access an attribute across all elements of the set?",
            at,
        ),
        t if t.is_primitive() => err(
            UNSUPPORTED,
            format!(
                "Can't access attributes on a primitive-typed value ({}).",
                t.friendly_name()
            ),
            at,
        ),
        _ => err(UNSUPPORTED, "This value does not have any attributes.", at),
    }
}
