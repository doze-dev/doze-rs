//! cty's values. A value is null or unknown of some type, or a known value of one of the types; known
//! collections hold their elements in cty's order (lists and tuples by index, maps and objects by key,
//! sets by cty's set ordering, without duplicates).

mod equality;
pub(crate) mod quote;
mod set;

use crate::number::Number;
use crate::types::{ObjectType, Type};
use std::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Null(Type),
    Unknown(Type),
    Bool(bool),
    Number(Number),
    /// Always NFC (cty normalizes every string it is given).
    String(String),
    /// Element type and elements; every element has exactly the element type.
    List(Type, Vec<Value>),
    /// Element type and elements, in cty's set order, no two equal.
    Set(Type, Vec<Value>),
    Map(Type, BTreeMap<String, Value>),
    Tuple(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

/// A value that cannot be built: elements whose types differ where they must match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch;

impl Value {
    /// The dynamic unknown (cty's `DynamicVal`).
    pub const DYNAMIC: Value = Value::Unknown(Type::Dynamic);

    pub fn string(s: &str) -> Value {
        Value::String(s.nfc().collect())
    }

    pub fn number(n: Number) -> Value {
        Value::Number(n)
    }

    pub fn int(i: i64) -> Value {
        Value::Number(Number::from_i64(i))
    }

    pub fn empty_tuple() -> Value {
        Value::Tuple(Vec::new())
    }

    pub fn empty_object() -> Value {
        Value::Object(BTreeMap::new())
    }

    /// A list of values that all have the same type (cty's `ListVal`), or an empty list of `ety`.
    pub fn list(ety: Type, elems: Vec<Value>) -> Result<Value, Mismatch> {
        let ety = same_type(ety, &elems)?;
        Ok(Value::List(ety, elems))
    }

    /// A set (cty's `SetVal`): equal elements collapse, the rest are put in cty's order.
    pub fn set(ety: Type, elems: Vec<Value>) -> Result<Value, Mismatch> {
        let ety = same_type(ety, &elems)?;
        Ok(Value::Set(ety.clone(), set::normalize(&ety, elems)))
    }

    /// A map of values that all have the same type (cty's `MapVal`), or an empty map of `ety`.
    pub fn map(ety: Type, elems: BTreeMap<String, Value>) -> Result<Value, Mismatch> {
        let first = elems.values().next().map(Value::ty);
        let ety = first.unwrap_or(ety);
        if elems.values().any(|v| v.ty() != ety) {
            return Err(Mismatch);
        }
        Ok(Value::Map(ety, elems))
    }

    pub fn ty(&self) -> Type {
        match self {
            Value::Null(t) | Value::Unknown(t) => t.clone(),
            Value::Bool(_) => Type::Bool,
            Value::Number(_) => Type::Number,
            Value::String(_) => Type::String,
            Value::List(e, _) => Type::list(e.clone()),
            Value::Set(e, _) => Type::set(e.clone()),
            Value::Map(e, _) => Type::map(e.clone()),
            Value::Tuple(es) => Type::Tuple(es.iter().map(Value::ty).collect()),
            Value::Object(attrs) => Type::Object(ObjectType::new(
                attrs.iter().map(|(k, v)| (k.clone(), v.ty())).collect(),
            )),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null(_))
    }

    /// Known at the top level (cty's `IsKnown`); elements may still be unknown.
    pub fn is_known(&self) -> bool {
        !matches!(self, Value::Unknown(_))
    }

    /// Known all the way down.
    pub fn is_wholly_known(&self) -> bool {
        match self {
            Value::Unknown(_) => false,
            Value::List(_, es) | Value::Set(_, es) | Value::Tuple(es) => {
                es.iter().all(Value::is_wholly_known)
            }
            Value::Map(_, m) | Value::Object(m) => m.values().all(Value::is_wholly_known),
            _ => true,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<&Number> {
        match self {
            Value::Number(n) => Some(n),
            _ => None,
        }
    }

    /// Whether the value is a known collection, tuple or object (cty's `CanIterateElements`).
    pub fn can_iterate(&self) -> bool {
        matches!(
            self.ty(),
            Type::List(_) | Type::Set(_) | Type::Map(_) | Type::Tuple(_) | Type::Object(_)
        )
    }

    /// Elements as (key, value) pairs in cty's iteration order: indexes for lists and tuples, names
    /// for maps and objects, and the element itself for sets. Empty for anything else.
    pub fn elements(&self) -> Vec<(Value, Value)> {
        match self {
            Value::List(_, es) | Value::Tuple(es) => es
                .iter()
                .enumerate()
                .map(|(i, v)| (Value::Number(Number::from_usize(i)), v.clone()))
                .collect(),
            Value::Set(_, es) => es.iter().map(|v| (v.clone(), v.clone())).collect(),
            Value::Map(_, m) | Value::Object(m) => m
                .iter()
                .map(|(k, v)| (Value::String(k.clone()), v.clone()))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The number of elements of a known collection, tuple or object.
    /// Whether a known collection, tuple or object has no elements (`None` for anything else).
    pub fn is_empty(&self) -> Option<bool> {
        self.len().map(|n| n == 0)
    }

    pub fn len(&self) -> Option<usize> {
        match self {
            Value::List(_, es) | Value::Set(_, es) | Value::Tuple(es) => Some(es.len()),
            Value::Map(_, m) | Value::Object(m) => Some(m.len()),
            _ => None,
        }
    }

    /// cty's `Equals`: a bool, or an unknown bool when unknowns stop it from deciding.
    pub fn equals(&self, other: &Value) -> Value {
        equality::equals(self, other)
    }

    /// cty's `RawEquals`: the same type and the same value, unknowns and nulls included.
    pub fn raw_equals(&self, other: &Value) -> bool {
        equality::raw_equals(self, other)
    }
}

/// The type every element shares: the first element's, or `ety` when there are none.
fn same_type(ety: Type, elems: &[Value]) -> Result<Type, Mismatch> {
    let Some(first) = elems.first() else {
        return Ok(ety);
    };
    let t = first.ty();
    if elems.iter().any(|v| v.ty() != t) {
        return Err(Mismatch);
    }
    Ok(t)
}
