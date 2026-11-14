//! cty's functions that build sequences or pick from them (`stdlib/sequence.go`, `collection.go`,
//! `general.go`): concat, flatten, slice, element, coalesce, range.

use super::{Spec, arg, list, panic_error, to_int};
use crate::convert::{convert, unify};
use crate::function::{FuncError, Param};
use crate::number::Number;
use crate::types::Type;
use crate::value::Value;
use std::cmp::Ordering;

pub(super) fn functions() -> Vec<(&'static str, Spec)> {
    let d = |name| Param::new(name, Type::Dynamic);
    let n = |name| Param::new(name, Type::Number);
    vec![
        (
            "concat",
            Spec::rule(vec![], concat_type, concat).var(d("seqs")),
        ),
        (
            "flatten",
            Spec::rule(vec![d("list")], flatten_type, flatten),
        ),
        (
            "slice",
            Spec::rule(
                vec![d("list"), n("start_index"), n("end_index")],
                slice_type,
                slice,
            ),
        ),
        (
            "element",
            Spec::rule(vec![d("list"), n("index")], element_type, element).refine(None),
        ),
        (
            "coalesce",
            Spec::rule(vec![], coalesce_type, coalesce).var(d("vals").unknown().dynamic().null()),
        ),
        (
            "range",
            Spec::new(vec![], Type::list(Type::Number), range).var(n("params")),
        ),
    ]
}

type Out = Result<Value, FuncError>;

fn concat_type(args: &[Value]) -> Result<Type, FuncError> {
    if args.is_empty() {
        return Err(FuncError::new("at least one argument is required"));
    }
    let tys: Vec<Type> = args.iter().map(Value::ty).collect();
    if tys.iter().all(|t| matches!(t, Type::List(_)))
        && let Some(t) = unify(&tys, true).ty()
    {
        return Ok(t);
    }
    let mut etys = Vec::new();
    for (i, a) in args.iter().enumerate() {
        match a.ty() {
            Type::Tuple(ts) => etys.extend(ts),
            Type::List(e) => {
                let Value::List(_, es) = a else {
                    return Ok(Type::Dynamic);
                };
                etys.extend(std::iter::repeat_n(*e, es.len()));
            }
            t => {
                return Err(FuncError::arg(
                    i,
                    format!(
                        "all arguments must be lists or tuples; got {}",
                        t.friendly_name()
                    ),
                ));
            }
        }
    }
    Ok(Type::Tuple(etys))
}

fn concat(args: &[Value], ret: &Type) -> Out {
    if let Type::List(ety) = ret {
        let mut vals = Vec::new();
        for (i, a) in args.iter().enumerate() {
            let l = convert(a, ret).map_err(|e| FuncError::arg(i, e.0))?;
            vals.extend(l.elements().into_iter().map(|(_, v)| v));
        }
        if vals.is_empty() {
            return Ok(Value::List((**ety).clone(), vals));
        }
        return list(Type::Dynamic, vals);
    }
    Ok(Value::Tuple(
        args.iter()
            .flat_map(|a| a.elements().into_iter().map(|(_, v)| v))
            .collect(),
    ))
}

/// Nested lists, sets and tuples, flattened; `None` while any of them is unknown.
fn flattener(v: &Value) -> Option<Vec<Value>> {
    if let Value::Set(_, es) = v
        && es.len() > 1
        && !v.is_wholly_known()
    {
        return None;
    }
    let mut out = Vec::new();
    let mut known = true;
    for (_, e) in v.elements() {
        if e == Value::DYNAMIC {
            known = false;
        }
        let seq = matches!(e.ty(), Type::List(_) | Type::Set(_) | Type::Tuple(_));
        if !e.is_null() && seq {
            match (e.is_known(), flattener(&e)) {
                (true, Some(inner)) => out.extend(inner),
                _ => known = false,
            }
        } else {
            out.push(e);
        }
    }
    known.then_some(out)
}

fn flatten_type(args: &[Value]) -> Result<Type, FuncError> {
    let v = arg(args, 0);
    if !v.is_wholly_known() {
        return Ok(Type::Dynamic);
    }
    if !matches!(v.ty(), Type::List(_) | Type::Set(_) | Type::Tuple(_)) {
        return Err(FuncError::new("can only flatten lists, sets and tuples"));
    }
    Ok(flattener(v).map_or(Type::Dynamic, |vs| {
        Type::Tuple(vs.iter().map(Value::ty).collect())
    }))
}

fn flatten(args: &[Value], ret: &Type) -> Out {
    let v = arg(args, 0);
    if v.len() == Some(0) {
        return Ok(Value::empty_tuple());
    }
    if !matches!(v.ty(), Type::List(_) | Type::Set(_) | Type::Tuple(_)) {
        return Err(panic_error("value is not a collection"));
    }
    Ok(flattener(v).map_or_else(|| Value::unknown(ret.clone()), Value::Tuple))
}

/// The start and end indexes that are known, checked against the length where that is known.
fn slice_indexes(args: &[Value]) -> Result<(Option<usize>, Option<usize>), FuncError> {
    let l = arg(args, 0);
    let len = match (l.ty(), l) {
        (Type::Tuple(ts), _) => Some(ts.len()),
        (_, Value::List(_, es)) => Some(es.len()),
        _ => None,
    };
    let index = |i: usize, what: &str| -> Result<Option<usize>, FuncError> {
        let v = arg(args, i);
        if !v.is_known() {
            return Ok(None);
        }
        let n = to_int(v).map_err(|e| FuncError::arg(i, format!("invalid {what} index: {e}")))?;
        let Ok(n) = usize::try_from(n) else {
            return Err(FuncError::arg(
                i,
                format!("{what} index must not be less than zero"),
            ));
        };
        if len.is_some_and(|l| n > l) {
            return Err(FuncError::arg(
                i,
                format!("{what} index must not be greater than the length of the list"),
            ));
        }
        Ok(Some(n))
    };
    let start = index(1, "start")?;
    let end = index(2, "end")?;
    if let (Some(s), Some(e)) = (start, end)
        && s > e
    {
        return Err(FuncError::arg(
            1,
            "start index must not be greater than end index",
        ));
    }
    Ok((start, end))
}

fn slice_type(args: &[Value]) -> Result<Type, FuncError> {
    let t = arg(args, 0).ty();
    if let Type::Set(_) = t {
        return Err(FuncError::arg(
            0,
            "cannot slice a set, because its elements do not have indices; explicitly convert to a list if the ordering of the result is not important",
        ));
    }
    if !matches!(t, Type::List(_) | Type::Tuple(_)) {
        return Err(FuncError::arg(0, "must be a list or tuple value"));
    }
    let (start, end) = slice_indexes(args)?;
    match (t, start, end) {
        (Type::Tuple(ts), Some(s), Some(e)) => {
            Ok(Type::Tuple(ts.get(s..e).unwrap_or_default().to_vec()))
        }
        (Type::Tuple(_), _, _) => Ok(Type::Dynamic),
        (t, _, _) => Ok(t),
    }
}

fn slice(args: &[Value], ret: &Type) -> Out {
    let (start, end) = slice_indexes(args)?;
    let (s, e) = (start.unwrap_or(0), end.unwrap_or(0));
    let vs: Vec<Value> = match arg(args, 0) {
        Value::List(_, es) | Value::Tuple(es) => es.get(s..e).unwrap_or_default().to_vec(),
        _ => Vec::new(),
    };
    match ret {
        Type::Tuple(_) => Ok(Value::Tuple(vs)),
        Type::List(ety) if vs.is_empty() => Ok(Value::List((**ety).clone(), vs)),
        Type::List(_) => list(Type::Dynamic, vs),
        _ => Ok(Value::DYNAMIC),
    }
}

/// Go's `index % len`, made non-negative.
fn wrap(index: i64, len: usize) -> usize {
    let l = i64::try_from(len).unwrap_or(i64::MAX).max(1);
    usize::try_from(index.rem_euclid(l)).unwrap_or(0)
}

fn element_type(args: &[Value]) -> Result<Type, FuncError> {
    match arg(args, 0).ty() {
        Type::List(e) => Ok(*e),
        Type::Tuple(ts) => {
            let idx = arg(args, 1);
            if !idx.is_known() {
                return Ok(Type::Dynamic);
            }
            let i = to_int(idx).map_err(|e| FuncError::new(format!("invalid index: {e}")))?;
            if ts.is_empty() {
                return Err(FuncError::new(
                    "cannot use element function with an empty list",
                ));
            }
            Ok(ts.get(wrap(i, ts.len())).cloned().unwrap_or(Type::Dynamic))
        }
        t => Err(FuncError::new(format!(
            "cannot read elements from {}",
            t.friendly_name()
        ))),
    }
}

fn element(args: &[Value], _: &Type) -> Out {
    let i = to_int(arg(args, 1)).map_err(|e| FuncError::new(format!("invalid index: {e}")))?;
    let es = match arg(args, 0) {
        Value::List(_, es) | Value::Tuple(es) => es,
        _ => return Err(panic_error("value is not a list or tuple")),
    };
    if es.is_empty() {
        return Err(FuncError::new(
            "cannot use element function with an empty list",
        ));
    }
    Ok(es.get(wrap(i, es.len())).cloned().unwrap_or(Value::DYNAMIC))
}

fn coalesce_type(args: &[Value]) -> Result<Type, FuncError> {
    let tys: Vec<Type> = args.iter().map(Value::ty).collect();
    unify(&tys, true)
        .ty()
        .ok_or_else(|| FuncError::new("all arguments must have the same type"))
}

fn coalesce(args: &[Value], ret: &Type) -> Out {
    for a in args {
        if !a.is_known() {
            return Ok(Value::unknown(ret.clone()));
        }
        if !a.is_null() {
            return convert(a, ret).map_err(|e| FuncError::new(e.0));
        }
    }
    Err(FuncError::new("no non-null arguments"))
}

/// cty's `range`: at most 1,024 values; a zero step is not caught early (cty compares it with its
/// own zero value by identity), so it counts to the limit.
fn range(args: &[Value], _: &Type) -> Out {
    let num = |i: usize| {
        arg(args, i)
            .as_number()
            .cloned()
            .unwrap_or_else(|| Number::from_i64(0))
    };
    let zero = Number::from_i64(0);
    let unit = |down: bool| Number::from_i64(if down { -1 } else { 1 });
    let (start, end, step) = match args.len() {
        1 => (
            zero.clone(),
            num(0),
            unit(num(0).cmp_value(&zero) == Ordering::Less),
        ),
        2 => (
            num(0),
            num(1),
            unit(num(1).cmp_value(&num(0)) == Ordering::Less),
        ),
        3 => (num(0), num(1), num(2)),
        _ => return Err(FuncError::new("must have one, two, or three arguments")),
    };
    let down = step.cmp_value(&zero) == Ordering::Less;
    let order = end.cmp_value(&start);
    if down && order == Ordering::Greater {
        return Err(FuncError::arg(
            1,
            "end must be less than start when step is negative",
        ));
    }
    if !down && order == Ordering::Less {
        return Err(FuncError::arg(
            1,
            "end must be greater than start when step is positive",
        ));
    }
    let mut vals = Vec::new();
    let mut n = start;
    loop {
        let o = n.cmp_value(&end);
        if (down && o != Ordering::Greater) || (!down && o != Ordering::Less) {
            break;
        }
        if vals.len() >= 1024 {
            return Err(FuncError::new(
                "more than 1024 values were generated; either decrease the difference between start and end or use a smaller step",
            ));
        }
        let next = n.add(&step).map_err(|e| panic_error(&e.to_string()))?;
        vals.push(Value::Number(n));
        n = next;
    }
    if vals.is_empty() {
        return Ok(Value::List(Type::Number, vals));
    }
    list(Type::Number, vals)
}
