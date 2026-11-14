//! cty's functions over collections that keep or inspect them whole (`stdlib/collection.go`,
//! `string.go`'s `sort`): length, contains, keys, values, lookup, merge, distinct, sort, reverse.

use super::{Spec, arg, list, map, panic_error, str_arg};
use crate::convert::convert;
use crate::function::{FuncError, Param};
use crate::number::Number;
use crate::types::Type;
use crate::value::{Refinement, Value};
use std::collections::BTreeMap;

pub(super) fn functions() -> Vec<(&'static str, Spec)> {
    let d = |name| Param::new(name, Type::Dynamic);
    vec![
        (
            "length",
            Spec::rule(
                vec![d("collection").dynamic().unknown()],
                length_type,
                length,
            ),
        ),
        (
            "contains",
            Spec::new(vec![d("list"), d("value").null()], Type::Bool, contains),
        ),
        (
            "keys",
            Spec::rule(vec![d("inputMap").unknown()], keys_type, keys),
        ),
        (
            "values",
            Spec::rule(vec![d("mapping")], values_type, values),
        ),
        (
            "lookup",
            Spec::rule(
                vec![d("inputMap"), Param::new("key", Type::String), d("default")],
                lookup_type,
                lookup,
            )
            .refine(None),
        ),
        (
            "merge",
            Spec::rule(vec![], merge_type, merge).var(d("maps").dynamic().null()),
        ),
        (
            "distinct",
            Spec::rule(
                vec![Param::new("list", Type::list(Type::Dynamic))],
                |args| Ok(arg(args, 0).ty()),
                distinct,
            ),
        ),
        (
            "sort",
            Spec::new(
                vec![Param::new("list", Type::list(Type::String)).unknown()],
                Type::list(Type::String),
                sort,
            ),
        ),
        (
            "reverse",
            Spec::rule(vec![d("list")], reverse_type, reverse),
        ),
    ]
}

type Out = Result<Value, FuncError>;

fn length_type(args: &[Value]) -> Result<Type, FuncError> {
    match arg(args, 0).ty() {
        Type::Tuple(_) | Type::List(_) | Type::Map(_) | Type::Set(_) | Type::Dynamic => {
            Ok(Type::Number)
        }
        _ => Err(FuncError::new(
            "collection must be a list, a map or a tuple",
        )),
    }
}

/// cty's `Length`: a tuple's from its type; an unknown's within its known length bounds; a set
/// holding unknowns somewhere between one and its stored count.
fn length(args: &[Value], _: &Type) -> Out {
    let v = arg(args, 0);
    let int = |n: usize| Number::from_usize(n);
    if let Type::Tuple(ts) = v.ty() {
        return Ok(Value::Number(int(ts.len())));
    }
    if let Value::Unknown(..) = v {
        let (lo, hi) = v.refinement().len.unwrap_or((0, None));
        let hi = hi.map_or_else(|| Number::from_i64(i64::MAX), int);
        return Ok(Value::refined(
            Type::Number,
            Refinement {
                lower: Some((int(lo), true)),
                upper: Some((hi, true)),
                ..Refinement::not_null()
            },
        ));
    }
    let n = v.len().unwrap_or(0);
    if matches!(v, Value::Set(..)) && n > 1 && !v.is_wholly_known() {
        return Ok(Value::refined(
            Type::Number,
            Refinement {
                lower: Some((int(1), true)),
                upper: Some((int(n), true)),
                ..Refinement::not_null()
            },
        ));
    }
    Ok(Value::Number(int(n)))
}

fn contains(args: &[Value], _: &Type) -> Out {
    let (coll, want) = (arg(args, 0), arg(args, 1));
    if !matches!(coll.ty(), Type::List(_) | Type::Tuple(_) | Type::Set(_)) {
        return Err(FuncError::new("argument must be list, tuple, or set"));
    }
    if coll.len() == Some(0) {
        return Ok(Value::Bool(false));
    }
    if !want.is_known() {
        return Ok(Value::unknown(Type::Bool));
    }
    let mut unknown = false;
    for (_, e) in coll.elements() {
        match want.equals(&e) {
            Value::Bool(true) => return Ok(Value::Bool(true)),
            Value::Bool(false) => {}
            _ => unknown = true,
        }
    }
    Ok(if unknown {
        Value::unknown(Type::Bool)
    } else {
        Value::Bool(false)
    })
}

fn keys_type(args: &[Value]) -> Result<Type, FuncError> {
    match arg(args, 0).ty() {
        Type::Map(_) => Ok(Type::list(Type::String)),
        Type::Object(o) => Ok(Type::Tuple(vec![Type::String; o.attrs.len()])),
        _ => Err(FuncError::arg(0, "must have map or object type")),
    }
}

fn keys(args: &[Value], ret: &Type) -> Out {
    let v = arg(args, 0);
    if let Type::Object(o) = v.ty() {
        return Ok(Value::Tuple(
            o.attrs.keys().map(|k| Value::string(k)).collect(),
        ));
    }
    if !v.is_known() {
        return Ok(Value::unknown(ret.clone()));
    }
    let ks: Vec<Value> = v.elements().into_iter().map(|(k, _)| k).collect();
    if ks.is_empty() {
        return Ok(Value::List(Type::String, Vec::new()));
    }
    list(Type::String, ks)
}

fn values_type(args: &[Value]) -> Result<Type, FuncError> {
    match arg(args, 0).ty() {
        Type::Map(e) => Ok(Type::List(e)),
        Type::Object(o) => Ok(Type::Tuple(o.attrs.into_values().collect())),
        _ => Err(FuncError::new(
            "values() requires a map as the first argument",
        )),
    }
}

fn values(args: &[Value], ret: &Type) -> Out {
    let vs: Vec<Value> = arg(args, 0)
        .elements()
        .into_iter()
        .map(|(_, v)| v)
        .collect();
    match ret {
        Type::Tuple(_) => Ok(Value::Tuple(vs)),
        _ if vs.is_empty() => Ok(Value::List(
            ret.element().cloned().unwrap_or(Type::Dynamic),
            vs,
        )),
        _ => list(Type::Dynamic, vs),
    }
}

fn lookup_type(args: &[Value]) -> Result<Type, FuncError> {
    let (m, key) = (arg(args, 0), arg(args, 1));
    match m.ty() {
        Type::Object(o) => {
            let Some(k) = key.as_str() else {
                return Ok(Type::Dynamic);
            };
            if let Some(t) = o.attrs.get(k) {
                return Ok(t.clone());
            }
            match args.get(2) {
                Some(d) => Ok(d.ty()),
                None => Err(FuncError::arg(
                    0,
                    format!(
                        "the given object has no attribute {}",
                        crate::value::quote::go_quote(k)
                    ),
                )),
            }
        }
        Type::Map(e) => {
            if let Some(d) = args.get(2)
                && convert(d, &e).is_err()
            {
                return Err(FuncError::arg(
                    2,
                    "the default value must have the same type as the map elements",
                ));
            }
            Ok(*e)
        }
        _ => Err(FuncError::arg(
            0,
            "lookup() requires a map as the first argument",
        )),
    }
}

fn lookup(args: &[Value], ret: &Type) -> Out {
    let (m, key) = (arg(args, 0), str_arg(args, 1));
    if !m.is_wholly_known() {
        return Ok(Value::unknown(ret.clone()));
    }
    let found = match m {
        Value::Object(attrs) | Value::Map(_, attrs) => attrs.get(key).cloned(),
        _ => None,
    };
    if let Some(v) = found {
        return Ok(v);
    }
    // cty's Impl reads `args[2]` without checking: a missing default is Go's index panic.
    let Some(d) = args.get(2) else {
        return Err(panic_error(
            "runtime error: index out of range [2] with length 2",
        ));
    };
    convert(d, ret).map_err(|e| FuncError::new(e.0))
}

/// The first argument's type when all match; otherwise an object with every attribute (a later
/// one's type winning), or the dynamic type while a map's keys are unknown.
fn merge_type(args: &[Value]) -> Result<Type, FuncError> {
    let mut attrs = BTreeMap::new();
    let mut first: Option<Type> = None;
    let (mut matching, mut keys_known) = (true, true);
    for a in args {
        let mut ty = a.ty();
        match &ty {
            Type::Dynamic => return Ok(Type::Dynamic),
            Type::Object(_) if a.is_null() => ty = Type::empty_object(),
            Type::Object(o) => attrs.extend(o.attrs.clone()),
            Type::Map(e) if a.is_known() => {
                for (k, _) in a.elements() {
                    attrs.insert(k.as_str().unwrap_or("").to_owned(), (**e).clone());
                }
            }
            Type::Map(_) => keys_known = false,
            t => {
                return Err(FuncError::new(format!(
                    "arguments must be maps or objects, got {}",
                    crate::value::quote::go_quote(&t.friendly_name())
                )));
            }
        }
        match &first {
            None => first = Some(ty),
            Some(f) => matching &= *f == ty,
        }
    }
    match first {
        None => Ok(Type::empty_object()),
        Some(f) if matching => Ok(f),
        _ if !keys_known => Ok(Type::Dynamic),
        _ => Ok(Type::object(attrs)),
    }
}

fn merge(args: &[Value], ret: &Type) -> Out {
    let mut out: BTreeMap<String, Value> = BTreeMap::new();
    // After an untyped null the type was not checked: Go iterates whatever comes, and panics on
    // what it cannot iterate or name.
    for a in args.iter().filter(|a| !a.is_null()) {
        if !a.can_iterate() {
            return Err(panic_error("attempt to iterate on non-collection"));
        }
        for (k, v) in a.elements() {
            let Some(key) = k.as_str() else {
                return Err(panic_error("not a string"));
            };
            out.insert(key.to_owned(), v);
        }
    }
    match ret {
        Type::Map(e) if out.is_empty() => Ok(Value::Map((**e).clone(), out)),
        Type::Map(e) => map((**e).clone(), out),
        _ => Ok(Value::Object(out)),
    }
}

fn distinct(args: &[Value], ret: &Type) -> Out {
    let v = arg(args, 0);
    if !v.is_wholly_known() {
        return Ok(Value::unknown(ret.clone()));
    }
    let mut kept: Vec<Value> = Vec::new();
    for (_, e) in v.elements() {
        if !kept.iter().any(|k| k.equals(&e) == Value::Bool(true)) {
            kept.push(e);
        }
    }
    let ety = ret.element().cloned().unwrap_or(Type::Dynamic);
    if kept.is_empty() {
        return Ok(Value::List(ety, kept));
    }
    list(ety, kept)
}

/// Byte-wise order (Go's `sort.Strings`); an unknown list keeps what is known of its length.
fn sort(args: &[Value], ret: &Type) -> Out {
    let v = arg(args, 0);
    if !v.is_wholly_known() {
        return Ok(Value::refined(
            ret.clone(),
            Refinement {
                len: v.refinement().len,
                ..Refinement::default()
            },
        ));
    }
    let mut items = Vec::new();
    for (i, (_, e)) in v.elements().into_iter().enumerate() {
        let Some(s) = e.as_str() else {
            return Err(FuncError::new(format!(
                "given list element {i} is null; a null string cannot be sorted"
            )));
        };
        items.push(s.to_owned());
    }
    items.sort();
    let vals: Vec<Value> = items.iter().map(|s| Value::string(s)).collect();
    if vals.is_empty() {
        return Ok(v.clone());
    }
    list(Type::String, vals)
}

fn reverse_type(args: &[Value]) -> Result<Type, FuncError> {
    match arg(args, 0).ty() {
        Type::Tuple(mut ts) => {
            ts.reverse();
            Ok(Type::Tuple(ts))
        }
        Type::List(e) | Type::Set(e) => Ok(Type::List(e)),
        t => Err(FuncError::arg(
            0,
            format!(
                "can only reverse list or tuple values, not {}",
                t.friendly_name()
            ),
        )),
    }
}

fn reverse(args: &[Value], ret: &Type) -> Out {
    let mut vs: Vec<Value> = arg(args, 0)
        .elements()
        .into_iter()
        .map(|(_, v)| v)
        .collect();
    vs.reverse();
    match ret {
        Type::Tuple(_) => Ok(Value::Tuple(vs)),
        _ if vs.is_empty() => Ok(Value::List(
            ret.element().cloned().unwrap_or(Type::Dynamic),
            vs,
        )),
        _ => list(Type::Dynamic, vs),
    }
}
