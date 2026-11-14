//! cty's `Equals` and `RawEquals`.

use super::Value;
use crate::types::Type;

fn unknown_bool() -> Value {
    Value::unknown_not_null(Type::Bool)
}

/// No unknown of the dynamic type anywhere inside (cty's `HasWhollyKnownType`).
fn wholly_known_type(v: &Value) -> bool {
    match v {
        Value::Null(_) => true,
        Value::Unknown(Type::Dynamic, _) => false,
        Value::Unknown(t, _) => {
            !(t.is_collection() || matches!(t, Type::Tuple(_) | Type::Object(_)))
                || !t.has_dynamic()
        }
        Value::List(_, es) | Value::Set(_, es) | Value::Tuple(es) => {
            es.iter().all(wholly_known_type)
        }
        Value::Map(_, m) | Value::Object(m) => m.values().all(wholly_known_type),
        _ => true,
    }
}

pub(super) fn equals(a: &Value, b: &Value) -> Value {
    if (a.is_null() && b.definitely_not_null()) || (b.is_null() && a.definitely_not_null()) {
        return Value::Bool(false);
    }
    // What is known of an unknown can already rule the other value out.
    if (a.is_known() && b.excludes(a)) || (b.is_known() && a.excludes(b)) {
        return Value::Bool(false);
    }
    match (a.is_known(), b.is_known()) {
        (false, false) => return unknown_bool(),
        (true, false) => return one_unknown(a, b),
        (false, true) => return one_unknown(b, a),
        (true, true) => {}
    }
    match (a.is_null(), b.is_null()) {
        (true, true) => return Value::Bool(true),
        (true, false) | (false, true) => return Value::Bool(false),
        (false, false) => {}
    }
    if !wholly_known_type(a) || !wholly_known_type(b) {
        let (ta, tb) = (a.ty(), b.ty());
        if !ta.conforms_to(&tb) && !tb.conforms_to(&ta) {
            return Value::Bool(false);
        }
        return unknown_bool();
    }
    if a.ty() != b.ty() {
        return Value::Bool(false);
    }
    known_equals(a, b)
}

fn one_unknown(known: &Value, unknown: &Value) -> Value {
    if known.is_null() || unknown.ty().has_dynamic() {
        return unknown_bool();
    }
    if known.ty() != unknown.ty() {
        return Value::Bool(false);
    }
    unknown_bool()
}

/// Both known, not null, the same type.
fn known_equals(a: &Value, b: &Value) -> Value {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => Value::Bool(x.same(y)),
        (Value::Bool(x), Value::Bool(y)) => Value::Bool(x == y),
        (Value::String(x), Value::String(y)) => Value::Bool(x == y),
        (Value::List(_, xs), Value::List(_, ys)) | (Value::Tuple(xs), Value::Tuple(ys)) => {
            if xs.len() != ys.len() {
                return Value::Bool(false);
            }
            all_equal(xs.iter().zip(ys))
        }
        (Value::Map(_, xs), Value::Map(_, ys)) | (Value::Object(xs), Value::Object(ys)) => {
            if xs.len() != ys.len() || xs.keys().ne(ys.keys()) {
                return Value::Bool(false);
            }
            all_equal(xs.values().zip(ys.values()))
        }
        (Value::Set(_, xs), Value::Set(_, ys)) => set_equals(xs, ys),
        _ => Value::Bool(false),
    }
}

/// True when every pair is equal; unknown as soon as one pair cannot be decided; false at the first
/// pair that differs (cty stops there).
fn all_equal<'a>(pairs: impl Iterator<Item = (&'a Value, &'a Value)>) -> Value {
    for (x, y) in pairs {
        match equals(x, y) {
            Value::Bool(true) => {}
            Value::Bool(false) => return Value::Bool(false),
            _ => return unknown_bool(),
        }
    }
    Value::Bool(true)
}

fn set_equals(xs: &[Value], ys: &[Value]) -> Value {
    if xs.iter().chain(ys).any(|v| !v.is_known()) {
        return unknown_bool();
    }
    let has = |set: &[Value], v: &Value| set.iter().any(|w| equals(v, w) == Value::Bool(true));
    Value::Bool(xs.iter().all(|x| has(ys, x)) && ys.iter().all(|y| has(xs, y)))
}

pub(super) fn raw_equals(a: &Value, b: &Value) -> bool {
    if a.ty() != b.ty() {
        return false;
    }
    match (a, b) {
        (Value::Unknown(_, a), Value::Unknown(_, b)) => a == b,
        (Value::Null(_), Value::Null(_)) => true,
        (Value::Unknown(..) | Value::Null(_), _) | (_, Value::Unknown(..) | Value::Null(_)) => {
            false
        }
        (Value::Number(x), Value::Number(y)) => x.same(y),
        (Value::List(_, xs), Value::List(_, ys))
        | (Value::Set(_, xs), Value::Set(_, ys))
        | (Value::Tuple(xs), Value::Tuple(ys)) => {
            xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| raw_equals(x, y))
        }
        (Value::Map(_, xs), Value::Map(_, ys)) | (Value::Object(xs), Value::Object(ys)) => {
            xs.len() == ys.len()
                && xs
                    .iter()
                    .zip(ys)
                    .all(|((ka, x), (kb, y))| ka == kb && raw_equals(x, y))
        }
        (x, y) => x == y,
    }
}
