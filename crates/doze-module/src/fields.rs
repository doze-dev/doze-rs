//! `Field` for the argument types a spec may use. Types are strict: the core has already converted values to the
//! schema's type, so a mismatch here is a bug in one side, reported as a diagnostic, never a panic.

use crate::spec::{Decoding, Field};
use doze_module_proto::msg::hello::Type;
use doze_module_proto::msg::value::{Node, PathSeg, Value};
use std::collections::BTreeMap;

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a bool",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::List(_) => "a list",
        Value::Map(_) => "an object",
    }
}

impl Field for String {
    fn ty() -> Type {
        Type::String
    }
    fn read(node: &Node, cx: &mut Decoding) -> Option<String> {
        match &node.value {
            Value::String(s) => Some(s.clone()),
            other => {
                cx.error(format!("expected a string, found {}", kind(other)), None);
                None
            }
        }
    }
}

impl Field for bool {
    fn ty() -> Type {
        Type::Bool
    }
    fn read(node: &Node, cx: &mut Decoding) -> Option<bool> {
        match &node.value {
            Value::Bool(b) => Some(*b),
            other => {
                cx.error(
                    format!("expected true or false, found {}", kind(other)),
                    None,
                );
                None
            }
        }
    }
}

macro_rules! number {
    ($($t:ty => $what:literal),*) => {$(
        impl Field for $t {
            fn ty() -> Type {
                Type::Number
            }
            fn read(node: &Node, cx: &mut Decoding) -> Option<$t> {
                match &node.value {
                    Value::Number(n) => match n.parse::<$t>() {
                        Ok(v) => Some(v),
                        Err(_) => {
                            cx.error(format!("{n} is not {}", $what), None);
                            None
                        }
                    },
                    other => {
                        cx.error(format!("expected a number, found {}", kind(other)), None);
                        None
                    }
                }
            }
        }
    )*};
}

number!(
    u16 => "a whole number from 0 to 65535",
    u32 => "a whole number from 0 to 4294967295",
    u64 => "a whole number of 0 or more",
    i32 => "a whole number in the 32-bit range",
    i64 => "a whole number in the 64-bit range",
    f64 => "a number"
);

impl<T: Field> Field for Option<T> {
    fn ty() -> Type {
        T::ty()
    }
    fn read(node: &Node, cx: &mut Decoding) -> Option<Option<T>> {
        match node.value {
            Value::Null => Some(None),
            _ => T::read(node, cx).map(Some),
        }
    }
    fn absent() -> Option<Option<T>> {
        Some(None)
    }
}

impl<T: Field> Field for Vec<T> {
    fn ty() -> Type {
        Type::List(Box::new(T::ty()))
    }
    fn read(node: &Node, cx: &mut Decoding) -> Option<Vec<T>> {
        let Value::List(items) = &node.value else {
            cx.error(
                format!("expected a list, found {}", kind(&node.value)),
                None,
            );
            return None;
        };
        let mut out = Vec::with_capacity(items.len());
        for (i, item) in items.iter().enumerate() {
            cx.path_push(PathSeg::Index(u32::try_from(i).unwrap_or(u32::MAX)));
            out.push(T::read(item, cx));
            cx.path_pop();
        }
        out.into_iter().collect()
    }
    fn absent() -> Option<Vec<T>> {
        Some(Vec::new())
    }
}

impl<T: Field> Field for BTreeMap<String, T> {
    fn ty() -> Type {
        Type::Map(Box::new(T::ty()))
    }
    fn read(node: &Node, cx: &mut Decoding) -> Option<BTreeMap<String, T>> {
        let Value::Map(m) = &node.value else {
            cx.error(
                format!("expected an object, found {}", kind(&node.value)),
                None,
            );
            return None;
        };
        let mut out = Vec::with_capacity(m.len());
        for (k, v) in m {
            cx.path_push(PathSeg::Key(k.clone()));
            out.push(T::read(v, cx).map(|v| (k.clone(), v)));
            cx.path_pop();
        }
        out.into_iter().collect()
    }
    fn absent() -> Option<BTreeMap<String, T>> {
        Some(BTreeMap::new())
    }
}
