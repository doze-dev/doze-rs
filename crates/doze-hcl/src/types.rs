//! cty's types. `Dynamic` is cty's `DynamicPseudoType`: "any type", the type of a value not yet
//! known and the wildcard in a type constraint.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Type {
    Dynamic,
    String,
    Number,
    Bool,
    List(Box<Type>),
    Set(Box<Type>),
    Map(Box<Type>),
    Tuple(Vec<Type>),
    Object(ObjectType),
}

/// An object type: attribute types by name, and (in a type constraint only) which are optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectType {
    pub attrs: BTreeMap<String, Type>,
    pub optional: BTreeSet<String>,
}

impl ObjectType {
    pub fn new(attrs: BTreeMap<String, Type>) -> ObjectType {
        ObjectType {
            attrs,
            optional: BTreeSet::new(),
        }
    }
}

impl Type {
    pub fn list(t: Type) -> Type {
        Type::List(Box::new(t))
    }

    pub fn set(t: Type) -> Type {
        Type::Set(Box::new(t))
    }

    pub fn map(t: Type) -> Type {
        Type::Map(Box::new(t))
    }

    pub fn object(attrs: BTreeMap<String, Type>) -> Type {
        Type::Object(ObjectType::new(attrs))
    }

    pub fn empty_object() -> Type {
        Type::Object(ObjectType::default())
    }

    pub fn is_primitive(&self) -> bool {
        matches!(self, Type::String | Type::Number | Type::Bool)
    }

    /// List, set or map.
    pub fn is_collection(&self) -> bool {
        matches!(self, Type::List(_) | Type::Set(_) | Type::Map(_))
    }

    /// The element type of a list, set or map.
    pub fn element(&self) -> Option<&Type> {
        match self {
            Type::List(e) | Type::Set(e) | Type::Map(e) => Some(e),
            _ => None,
        }
    }

    /// Whether `Dynamic` appears anywhere in the type.
    pub fn has_dynamic(&self) -> bool {
        match self {
            Type::Dynamic => true,
            Type::String | Type::Number | Type::Bool => false,
            Type::List(e) | Type::Set(e) | Type::Map(e) => e.has_dynamic(),
            Type::Tuple(es) => es.iter().any(Type::has_dynamic),
            Type::Object(o) => o.attrs.values().any(Type::has_dynamic),
        }
    }

    /// The type with optional-attribute markers removed, at every depth (values never carry them).
    pub fn without_optional(&self) -> Type {
        match self {
            Type::List(e) => Type::list(e.without_optional()),
            Type::Set(e) => Type::set(e.without_optional()),
            Type::Map(e) => Type::map(e.without_optional()),
            Type::Tuple(es) => Type::Tuple(es.iter().map(Type::without_optional).collect()),
            Type::Object(o) => Type::object(
                o.attrs
                    .iter()
                    .map(|(k, t)| (k.clone(), t.without_optional()))
                    .collect(),
            ),
            t => t.clone(),
        }
    }

    /// cty's `FriendlyName`, used in messages.
    pub fn friendly_name(&self) -> String {
        match self {
            Type::Dynamic => "dynamic".into(),
            Type::String => "string".into(),
            Type::Number => "number".into(),
            Type::Bool => "bool".into(),
            Type::List(e) => format!("list of {}", e.friendly_name()),
            Type::Set(e) => format!("set of {}", e.friendly_name()),
            Type::Map(e) => format!("map of {}", e.friendly_name()),
            Type::Tuple(_) => "tuple".into(),
            Type::Object(_) => "object".into(),
        }
    }

    /// cty's `TestConformance`: whether a value of this type fits where `want` is asked for, with
    /// `Dynamic` in `want` matching anything.
    pub fn conforms_to(&self, want: &Type) -> bool {
        match (self, want) {
            (_, Type::Dynamic) => true,
            (Type::List(a), Type::List(b))
            | (Type::Set(a), Type::Set(b))
            | (Type::Map(a), Type::Map(b)) => a.conforms_to(b),
            (Type::Tuple(a), Type::Tuple(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.conforms_to(y))
            }
            (Type::Object(a), Type::Object(b)) => {
                a.attrs.len() == b.attrs.len()
                    && a.attrs
                        .iter()
                        .all(|(k, t)| b.attrs.get(k).is_some_and(|w| t.conforms_to(w)))
            }
            (a, b) => a == b,
        }
    }

    /// cty's JSON encoding of a type (`ctyjson.MarshalType`), as the oracle writes it.
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        self.write_json(&mut out);
        out
    }

    fn write_json(&self, out: &mut String) {
        let wrap = |out: &mut String, kind: &str, e: &Type| {
            out.push_str("[\"");
            out.push_str(kind);
            out.push_str("\",");
            e.write_json(out);
            out.push(']');
        };
        match self {
            Type::Dynamic => out.push_str("\"dynamic\""),
            Type::String => out.push_str("\"string\""),
            Type::Number => out.push_str("\"number\""),
            Type::Bool => out.push_str("\"bool\""),
            Type::List(e) => wrap(out, "list", e),
            Type::Set(e) => wrap(out, "set", e),
            Type::Map(e) => wrap(out, "map", e),
            Type::Tuple(es) => {
                out.push_str("[\"tuple\",[");
                for (i, e) in es.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    e.write_json(out);
                }
                out.push_str("]]");
            }
            Type::Object(o) => o.write_json(out),
        }
    }
}

impl ObjectType {
    fn write_json(&self, out: &mut String) {
        out.push_str("[\"object\",{");
        for (i, (k, t)) in self.attrs.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            crate::value::quote::json_string(out, k);
            out.push(':');
            t.write_json(out);
        }
        out.push('}');
        if !self.optional.is_empty() {
            out.push_str(",[");
            for (i, k) in self.optional.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                crate::value::quote::json_string(out, k);
            }
            out.push(']');
        }
        out.push(']');
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.friendly_name())
    }
}
