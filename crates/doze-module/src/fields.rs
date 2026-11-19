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

/// An object whose entries keep the order they were written in: a doze.hcl object travels as a map (keys sorted), but
/// each value carries its span, so the order of the source is the order of the spans. For a module to which order is
/// meaning (a compound index's keys). When the order cannot be read (two entries at one place or in two files, as an
/// object the host computed would be), decoding fails at the object rather than guess: write it out literally.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Ordered<T>(pub Vec<(String, T)>);

impl<T> Ordered<T> {
    /// The entries, in the order they were written.
    pub fn iter(&self) -> impl Iterator<Item = &(String, T)> {
        self.0.iter()
    }
}

impl<T: Field> Field for Ordered<T> {
    fn ty() -> Type {
        Type::Map(Box::new(T::ty()))
    }
    fn read(node: &Node, cx: &mut Decoding) -> Option<Ordered<T>> {
        let Value::Map(m) = &node.value else {
            cx.error(
                format!("expected an object, found {}", kind(&node.value)),
                None,
            );
            return None;
        };
        let mut entries: Vec<(&String, &Node)> = m.iter().collect();
        entries.sort_by_key(|(_, v)| (v.span.file, v.span.start));
        let unreadable = entries.windows(2).any(|w| match w {
            [(_, a), (_, b)] => a.span.file != b.span.file || a.span.start == b.span.start,
            _ => false,
        });
        if unreadable {
            cx.error(
                "the order of this object's entries cannot be read (it was not written out where it is used)",
                Some("write the object literally here, e.g. { a = 1, b = -1 }".into()),
            );
            return None;
        }
        let mut out = Vec::with_capacity(entries.len());
        for (k, v) in entries {
            cx.path_push(PathSeg::Key(k.clone()));
            out.push(T::read(v, cx).map(|v| (k.clone(), v)));
            cx.path_pop();
        }
        out.into_iter().collect::<Option<Vec<_>>>().map(Ordered)
    }
    fn absent() -> Option<Ordered<T>> {
        Some(Ordered(Vec::new()))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::Ordered;
    use crate::spec::{Decoding, Field};
    use doze_module_proto::msg::value::{Node, Span, Value};
    use std::collections::BTreeMap;

    fn at(file: u16, start: u32, n: i64) -> Node {
        Node {
            span: Span {
                file,
                start,
                end: start + 1,
            },
            value: Value::Number(n.to_string()),
        }
    }

    fn object(entries: &[(&str, Node)]) -> Node {
        let m: BTreeMap<String, Node> = entries
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect();
        Node {
            span: Span::default(),
            value: Value::Map(m),
        }
    }

    #[test]
    fn entries_come_back_in_the_order_written() {
        let node = object(&[
            ("status", at(1, 10, 1)),
            ("at", at(1, 24, -1)),
            ("b", at(1, 40, 1)),
        ]);
        let mut cx = Decoding::new("");
        let got: Ordered<i64> = Field::read(&node, &mut cx).unwrap();
        let keys: Vec<&str> = got.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["status", "at", "b"]);
        assert!(cx.diagnostics.is_empty());
    }

    #[test]
    fn an_order_that_cannot_be_read_is_refused_not_guessed() {
        for node in [
            // The host made it: no spans.
            object(&[("b", at(0, 0, 1)), ("a", at(0, 0, 1))]),
            // Two files.
            object(&[("b", at(1, 5, 1)), ("a", at(2, 1, 1))]),
        ] {
            let mut cx = Decoding::new("");
            assert!(<Ordered<i64> as Field>::read(&node, &mut cx).is_none());
            assert_eq!(cx.diagnostics.len(), 1);
        }
        // One entry has an order whatever its span.
        let mut cx = Decoding::new("");
        let one: Ordered<i64> = Field::read(&object(&[("a", at(0, 0, 1))]), &mut cx).unwrap();
        assert_eq!(one.0.len(), 1);
    }
}
