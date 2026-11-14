//! Type conversion with go-cty's rules (`cty/convert`): which conversions exist ("safe" ones never
//! lose information; "unsafe" ones, like string to number, may fail on a value), and how a value is
//! converted, nulls and unknowns included.

mod message;
mod sort;
mod structural;
mod unify;

pub use message::mismatch;
pub use unify::{Unified, unify};

use crate::number::Number;
use crate::types::Type;
use crate::value::Value;

/// A value that cannot be converted, and why (cty's error text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvError(pub String);

impl std::fmt::Display for ConvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn err(msg: impl Into<String>) -> ConvError {
    ConvError(msg.into())
}

/// cty's `convert.Convert`: the unsafe conversion of `v` to `want`.
pub fn convert(v: &Value, want: &Type) -> Result<Value, ConvError> {
    let from = v.ty();
    if from == want.without_optional() {
        return Ok(v.clone());
    }
    if !exists(&from, want, true) {
        return Err(err(mismatch(&from, want)));
    }
    apply(v, want, true)
}

/// Whether cty's `getConversion(from, to, unsafe)` finds a conversion. An identical type converts to
/// itself (every caller in cty checks for that first).
pub(crate) fn exists(from: &Type, to: &Type, unsafe_: bool) -> bool {
    if from == to {
        return true;
    }
    match (from, to) {
        (_, Type::Dynamic) => true,
        (Type::Dynamic, _) => unsafe_,
        (Type::Number | Type::Bool, Type::String) => true,
        (Type::String, Type::Number | Type::Bool) => unsafe_,
        (a, b) if a.is_primitive() && b.is_primitive() => false,
        _ => structural::exists(from, to, unsafe_),
    }
}

/// Applies a conversion that `exists`: nulls and unknowns become nulls and unknowns of the target
/// type (with any `Dynamic` in it filled from the source type), everything else is converted.
pub(crate) fn apply(v: &Value, to: &Type, unsafe_: bool) -> Result<Value, ConvError> {
    let from = v.ty();
    if *to == Type::Dynamic || from == *to {
        return Ok(v.clone());
    }
    match v {
        Value::Unknown(_) => Ok(Value::Unknown(dynamic_replace(
            &from,
            &to.without_optional(),
        ))),
        Value::Null(_) => Ok(Value::Null(dynamic_replace(&from, &to.without_optional()))),
        _ => apply_known(v, &from, to, unsafe_),
    }
}

fn apply_known(v: &Value, from: &Type, to: &Type, unsafe_: bool) -> Result<Value, ConvError> {
    if *from == Type::Dynamic {
        return convert(v, to);
    }
    match (v, to) {
        (Value::Number(n), Type::String) => Ok(Value::String(n.text())),
        (Value::Bool(b), Type::String) => Ok(Value::string(if *b { "true" } else { "false" })),
        (Value::String(s), Type::Number) => Number::parse(s)
            .map(Value::Number)
            .ok_or_else(|| err("a number is required")),
        (Value::String(s), Type::Bool) => string_to_bool(s),
        _ => structural::apply_structural(v, from, to, unsafe_),
    }
}

fn string_to_bool(s: &str) -> Result<Value, ConvError> {
    match s {
        "true" | "1" => Ok(Value::Bool(true)),
        "false" | "0" => Ok(Value::Bool(false)),
        _ => match s.to_lowercase().as_str() {
            "true" => Err(err(
                "a bool is required; to convert from string, use lowercase \"true\"",
            )),
            "false" => Err(err(
                "a bool is required; to convert from string, use lowercase \"false\"",
            )),
            _ => Err(err("a bool is required")),
        },
    }
}

/// cty's `dynamicReplace`: the target type with each `Dynamic` in it replaced by what the source type
/// has in that place, so a null or unknown keeps as much type as it had.
pub(crate) fn dynamic_replace(from: &Type, to: &Type) -> Type {
    if *from == Type::Dynamic {
        return to.clone();
    }
    match to {
        Type::Dynamic => from.clone(),
        Type::String | Type::Number | Type::Bool => to.clone(),
        Type::Map(e) => match from {
            Type::Map(fe) => Type::map(dynamic_replace(fe, e)),
            Type::Object(o) => {
                let tys: Vec<Type> = o.attrs.values().cloned().collect();
                Type::map(dynamic_replace(&unify(&tys, true).ty_or_nil(), e))
            }
            _ => to.clone(),
        },
        Type::Object(o) => match from {
            Type::Map(fe) => Type::object(
                o.attrs
                    .iter()
                    .map(|(k, t)| (k.clone(), dynamic_replace(fe, t)))
                    .collect(),
            ),
            Type::Object(fo) => Type::object(
                o.attrs
                    .iter()
                    .map(|(k, t)| {
                        (
                            k.clone(),
                            fo.attrs
                                .get(k)
                                .map_or_else(|| t.clone(), |ft| dynamic_replace(ft, t)),
                        )
                    })
                    .collect(),
            ),
            _ => Type::empty_object(),
        },
        Type::Set(e) | Type::List(e) => {
            let inner = match from {
                Type::Set(fe) | Type::List(fe) => dynamic_replace(fe, e),
                Type::Tuple(ts) => dynamic_replace(&unify(ts, true).ty_or_nil(), e),
                _ => return to.clone(),
            };
            if matches!(to, Type::Set(_)) {
                Type::set(inner)
            } else {
                Type::list(inner)
            }
        }
        Type::Tuple(ts) => {
            let fs: &[Type] = if let Type::Tuple(fs) = from { fs } else { &[] };
            Type::Tuple(
                ts.iter()
                    .enumerate()
                    .map(|(i, t)| dynamic_replace(fs.get(i).unwrap_or(&Type::Dynamic), t))
                    .collect(),
            )
        }
    }
}
