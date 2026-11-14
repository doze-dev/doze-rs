//! Conversions between collections and structural types (cty's `conversion_collection.go`,
//! `conversion_object.go`, `conversion_tuple.go`).

use super::{ConvError, apply, err, exists as conv_exists, unify};
use crate::types::{ObjectType, Type};
use crate::value::Value;
use std::collections::BTreeMap;

pub(super) fn exists(from: &Type, to: &Type, unsafe_: bool) -> bool {
    match (from, to) {
        (Type::Object(a), Type::Object(b)) => object_to_object(a, b, unsafe_),
        (Type::Tuple(a), Type::Tuple(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| conv_exists(x, y, unsafe_))
        }
        (Type::List(a) | Type::Set(a), Type::List(b)) => conv_exists(a, b, unsafe_),
        (Type::List(_), Type::Set(_)) if !unsafe_ => false,
        (Type::List(a) | Type::Set(a), Type::Set(b)) => conv_exists(a, b, unsafe_),
        (Type::Map(a), Type::Map(b)) => conv_exists(a, b, unsafe_),
        (Type::Tuple(ts), Type::List(e) | Type::Set(e)) => {
            tuple_to_collection_ety(ts, e, unsafe_).is_some()
        }
        (Type::Object(o), Type::Map(e)) => object_to_map_ety(o, e, unsafe_).is_some(),
        (Type::Map(e), Type::Object(o)) => {
            unsafe_
                && o.attrs.iter().all(|(k, t)| {
                    e.as_ref() == t || conv_exists(e, t, unsafe_) || o.optional.contains(k)
                })
        }
        _ => false,
    }
}

fn object_to_object(a: &ObjectType, b: &ObjectType, unsafe_: bool) -> bool {
    b.attrs.iter().all(|(k, bt)| match a.attrs.get(k) {
        None => b.optional.contains(k),
        Some(at) => conv_exists(at, bt, unsafe_),
    })
}

/// The element type a tuple converts to a list or set with, if it can (cty's
/// `conversionTupleToList`/`ToSet` set-up).
fn tuple_to_collection_ety(ts: &[Type], ety: &Type, unsafe_: bool) -> Option<Type> {
    if ts.is_empty() {
        return Some(ety.without_optional());
    }
    let ety = if *ety == Type::Dynamic {
        let u = unify(ts, unsafe_).ty()?;
        if u == Type::Dynamic && ts.iter().any(|t| *t != Type::Dynamic) {
            return None;
        }
        u
    } else {
        ety.clone()
    };
    ts.iter()
        .all(|t| conv_exists(t, &ety, unsafe_))
        .then_some(ety)
}

fn object_to_map_ety(o: &ObjectType, ety: &Type, unsafe_: bool) -> Option<Type> {
    if o.attrs.is_empty() {
        return Some(ety.without_optional());
    }
    let ety = if *ety == Type::Dynamic {
        let tys: Vec<Type> = o.attrs.values().cloned().collect();
        unify(&tys, unsafe_).ty()?
    } else {
        ety.clone()
    };
    o.attrs
        .values()
        .all(|t| conv_exists(t, &ety, unsafe_))
        .then_some(ety)
}

/// A null element loses any optional-attribute markers on its type.
fn norm_null(v: Value) -> Value {
    match v {
        Value::Null(t) => Value::Null(t.without_optional()),
        v => v,
    }
}

pub(super) fn apply_structural(
    v: &Value,
    from: &Type,
    to: &Type,
    unsafe_: bool,
) -> Result<Value, ConvError> {
    match (v, to) {
        (Value::Object(attrs), Type::Object(o)) => to_object(attrs, o, unsafe_),
        (Value::Tuple(es), Type::Tuple(ts)) => {
            let out = es
                .iter()
                .zip(ts)
                .map(|(e, t)| apply(e, t, unsafe_))
                .collect::<Result<_, _>>()?;
            Ok(Value::Tuple(out))
        }
        // A set holding unknowns has an unknown length, so its list is unknown (cty's
        // `conversionCollectionToList`, which keeps the set's element type).
        (Value::Set(ety, es), Type::List(_))
            if es.len() > 1 && es.iter().any(|e| !e.is_known()) =>
        {
            Ok(Value::unknown(Type::list(ety.clone())))
        }
        (Value::List(ety, es) | Value::Set(ety, es), Type::List(want)) => {
            let out = convert_each(es, want, unsafe_)?;
            let empty_ety = if **want == Type::Dynamic {
                ety.clone()
            } else {
                want.without_optional()
            };
            Value::list(empty_ety, out)
                .map_err(|_| err("element types must all match for conversion to list"))
        }
        (Value::List(ety, es) | Value::Set(ety, es), Type::Set(want)) => {
            let out = convert_each(es, want, unsafe_)?;
            let empty_ety = if **want == Type::Dynamic {
                ety.clone()
            } else {
                want.without_optional()
            };
            Value::set(empty_ety, out)
                .map_err(|_| err("element types must all match for conversion to set"))
        }
        (Value::Map(ety, m), Type::Map(want)) => map_to_map(ety, m, want, unsafe_),
        (Value::Tuple(es), Type::List(want) | Type::Set(want)) => {
            let Type::Tuple(ts) = from else {
                return Err(err("tuple required"));
            };
            let ety = tuple_to_collection_ety(ts, want, unsafe_)
                .ok_or_else(|| err(super::mismatch(from, to)))?;
            tuple_to_collection(es, &ety, matches!(to, Type::Set(_)), unsafe_)
        }
        (Value::Object(attrs), Type::Map(want)) => {
            let Type::Object(o) = from else {
                return Err(err("object required"));
            };
            let ety = object_to_map_ety(o, want, unsafe_)
                .ok_or_else(|| err(super::mismatch(from, to)))?;
            object_to_map(attrs, &ety, unsafe_)
        }
        (Value::Map(ety, m), Type::Object(o)) => map_to_object(ety, m, o, unsafe_),
        _ => Err(err(super::mismatch(from, to))),
    }
}

fn convert_each(es: &[Value], want: &Type, unsafe_: bool) -> Result<Vec<Value>, ConvError> {
    es.iter()
        .map(|e| apply(e, want, unsafe_).map(norm_null))
        .collect()
}

fn to_object(
    attrs: &BTreeMap<String, Value>,
    o: &ObjectType,
    unsafe_: bool,
) -> Result<Value, ConvError> {
    let mut out = BTreeMap::new();
    for (k, v) in attrs {
        if let Some(want) = o.attrs.get(k) {
            out.insert(k.clone(), norm_null(apply(v, want, unsafe_)?));
        }
    }
    for k in &o.optional {
        if let (false, Some(t)) = (out.contains_key(k), o.attrs.get(k)) {
            out.insert(k.clone(), Value::Null(t.without_optional()));
        }
    }
    Ok(Value::Object(out))
}

fn map_to_map(
    ety: &Type,
    m: &BTreeMap<String, Value>,
    want: &Type,
    unsafe_: bool,
) -> Result<Value, ConvError> {
    let mut out = BTreeMap::new();
    for (k, v) in m {
        out.insert(k.clone(), apply(v, want, unsafe_)?);
    }
    if out.is_empty() {
        let e = if *want == Type::Dynamic {
            ety.clone()
        } else {
            want.without_optional()
        };
        return Ok(Value::Map(e, out));
    }
    if want.is_collection() || matches!(want, Type::Object(_)) {
        out = unify_values(out, false)?;
    }
    Value::map(Type::Dynamic, out)
        .map_err(|_| err("element types must all match for conversion to map"))
}

fn tuple_to_collection(
    es: &[Value],
    ety: &Type,
    set: bool,
    unsafe_: bool,
) -> Result<Value, ConvError> {
    if es.is_empty() {
        return Ok(if set {
            Value::Set(ety.clone(), Vec::new())
        } else {
            Value::List(ety.clone(), Vec::new())
        });
    }
    if set {
        let out = convert_each(es, ety, unsafe_)?;
        return Value::set(Type::Dynamic, out)
            .map_err(|_| err("element types must all match for conversion to set"));
    }
    let out: Vec<Value> = es
        .iter()
        .map(|e| apply(e, ety, unsafe_))
        .collect::<Result<_, _>>()?;
    let keyed: BTreeMap<usize, Value> = out.into_iter().enumerate().collect();
    let out: Vec<Value> = unify_indexed(keyed, unsafe_)?.into_values().collect();
    Value::list(Type::Dynamic, out)
        .map_err(|_| err("element types must all match for conversion to list"))
}

fn object_to_map(
    attrs: &BTreeMap<String, Value>,
    ety: &Type,
    unsafe_: bool,
) -> Result<Value, ConvError> {
    if attrs.is_empty() {
        return Ok(Value::Map(ety.clone(), BTreeMap::new()));
    }
    let mut out = BTreeMap::new();
    for (k, v) in attrs {
        out.insert(k.clone(), apply(v, ety, unsafe_)?);
    }
    if ety.is_collection() || matches!(ety, Type::Object(_)) {
        out = unify_values(out, unsafe_)?;
    }
    Value::map(Type::Dynamic, out)
        .map_err(|_| err("attribute types must all match for conversion to map"))
}

fn map_to_object(
    ety: &Type,
    m: &BTreeMap<String, Value>,
    o: &ObjectType,
    unsafe_: bool,
) -> Result<Value, ConvError> {
    let mut out = BTreeMap::new();
    for (k, v) in m {
        let Some(want) = o.attrs.get(k) else { continue };
        if want != ety && !conv_exists(ety, want, unsafe_) {
            return Err(err(format!(
                "map element type is incompatible with attribute {}: {}",
                crate::value::quote::go_quote(k),
                super::mismatch(&v.ty(), want)
            )));
        }
        out.insert(k.clone(), norm_null(apply(v, want, unsafe_)?));
    }
    for (k, t) in &o.attrs {
        if out.contains_key(k) {
            continue;
        }
        if !o.optional.contains(k) {
            return Err(err(format!(
                "map has no element for required attribute {}",
                crate::value::quote::go_quote(k)
            )));
        }
        out.insert(k.clone(), Value::Null(t.clone()));
    }
    Ok(Value::Object(out))
}

/// cty's `conversionUnifyCollectionElements`: converts every element to one type they all reach.
fn unify_values(
    elems: BTreeMap<String, Value>,
    unsafe_: bool,
) -> Result<BTreeMap<String, Value>, ConvError> {
    let tys: Vec<Type> = elems.values().map(Value::ty).collect();
    let target = unify(&tys, unsafe_)
        .ty()
        .ok_or_else(|| err("cannot find a common base type for all elements"))?;
    elems
        .into_iter()
        .map(|(k, v)| apply(&v, &target, unsafe_).map(|v| (k, v)))
        .collect()
}

fn unify_indexed(
    elems: BTreeMap<usize, Value>,
    unsafe_: bool,
) -> Result<BTreeMap<usize, Value>, ConvError> {
    let tys: Vec<Type> = elems.values().map(Value::ty).collect();
    let target = unify(&tys, unsafe_)
        .ty()
        .ok_or_else(|| err("cannot find a common base type for all elements"))?;
    elems
        .into_iter()
        .map(|(k, v)| apply(&v, &target, unsafe_).map(|v| (k, v)))
        .collect()
}
