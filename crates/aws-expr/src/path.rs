//! Reading and writing the places a document path names inside an item.

use crate::ast::{Elem, Path};
use crate::value::{Item, Value};

/// The value at `path`, if there is one.
#[must_use]
pub fn get<'a>(item: &'a Item, path: &Path) -> Option<&'a Value> {
    let (first, rest) = path.0.split_first()?;
    let Elem::Key(root) = first else { return None };
    rest.iter()
        .try_fold(item.get(root)?, |at, elem| step(at, elem))
}

fn step<'a>(at: &'a Value, elem: &Elem) -> Option<&'a Value> {
    match (at, elem) {
        (Value::M(m), Elem::Key(k)) => m.get(k),
        (Value::L(l), Elem::Index(i)) => l.get(*i),
        _ => None,
    }
}

fn step_mut<'a>(at: &'a mut Value, elem: &Elem) -> Option<&'a mut Value> {
    match (at, elem) {
        (Value::M(m), Elem::Key(k)) => m.get_mut(k),
        (Value::L(l), Elem::Index(i)) => l.get_mut(*i),
        _ => None,
    }
}

/// The existing value at the elements `elems` (a path's first element names an attribute of the item).
fn locate_mut<'a>(item: &'a mut Item, elems: &[Elem]) -> Option<&'a mut Value> {
    let (first, rest) = elems.split_first()?;
    let Elem::Key(root) = first else { return None };
    rest.iter()
        .try_fold(item.get_mut(root)?, |at, elem| step_mut(at, elem))
}

/// Puts `value` at `path`. A map key is added or replaced; a list index inside the list replaces, and one past the end appends. The
/// parent must exist (`false` when it does not, or is not a map or list as the step needs).
pub(crate) fn set(item: &mut Item, path: &Path, value: Value) -> bool {
    let Some((last, parent)) = path.0.split_last() else {
        return false;
    };
    if parent.is_empty() {
        let Elem::Key(k) = last else { return false };
        item.insert(k.clone(), value);
        return true;
    }
    match (locate_mut(item, parent), last) {
        (Some(Value::M(m)), Elem::Key(k)) => {
            m.insert(k.clone(), value);
            true
        }
        (Some(Value::L(l)), Elem::Index(i)) => {
            match l.get_mut(*i) {
                Some(slot) => *slot = value,
                None => l.push(value),
            }
            true
        }
        _ => false,
    }
}

/// Removes what is at `path`, if anything is: a missing path is not an error. Removing a list element shifts the rest down.
pub(crate) fn remove(item: &mut Item, path: &Path) {
    let Some((last, parent)) = path.0.split_last() else {
        return;
    };
    if parent.is_empty() {
        if let Elem::Key(k) = last {
            item.remove(k);
        }
        return;
    }
    match (locate_mut(item, parent), last) {
        (Some(Value::M(m)), Elem::Key(k)) => {
            m.remove(k);
        }
        (Some(Value::L(l)), Elem::Index(i)) if *i < l.len() => {
            l.remove(*i);
        }
        _ => {}
    }
}

/// A mutable view of the value at `path`, if it exists.
pub(crate) fn get_mut<'a>(item: &'a mut Item, path: &Path) -> Option<&'a mut Value> {
    locate_mut(item, &path.0)
}

/// The paths' values copied into a new item holding only them, each in the same place (a projection).
#[must_use]
pub fn project(item: &Item, paths: &[Path]) -> Item {
    let mut out = Item::new();
    for path in paths {
        if let Some(v) = get(item, path) {
            insert_nested(&mut out, &path.0, v.clone());
        }
    }
    out
}

/// Builds the maps and lists on the way to `value`. A list element projected alone becomes a one-element list.
fn insert_nested(out: &mut Item, elems: &[Elem], value: Value) {
    let Some((Elem::Key(root), rest)) = elems.split_first() else {
        return;
    };
    let merged = match out.remove(root) {
        Some(existing) => merge(existing, rest, value),
        None => wrap(rest, value),
    };
    out.insert(root.clone(), merged);
}

fn wrap(rest: &[Elem], value: Value) -> Value {
    match rest.split_first() {
        None => value,
        Some((Elem::Key(k), tail)) => Value::M([(k.clone(), wrap(tail, value))].into()),
        Some((Elem::Index(_), tail)) => Value::L(vec![wrap(tail, value)]),
    }
}

fn merge(existing: Value, rest: &[Elem], value: Value) -> Value {
    match (existing, rest.split_first()) {
        (Value::M(mut m), Some((Elem::Key(k), tail))) => {
            let next = match m.remove(k) {
                Some(inner) => merge(inner, tail, value),
                None => wrap(tail, value),
            };
            m.insert(k.clone(), next);
            Value::M(m)
        }
        (Value::L(mut l), Some((Elem::Index(_), tail))) => {
            l.push(wrap(tail, value));
            Value::L(l)
        }
        (other, _) => other,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::value::item_from_json;

    fn p(elems: &[Elem]) -> Path {
        Path(elems.to_vec())
    }

    fn k(s: &str) -> Elem {
        Elem::Key(s.into())
    }

    fn doc() -> Item {
        item_from_json(
            &json!({"a": {"M": {"b": {"L": [{"S": "x"}, {"S": "y"}]}}}, "n": {"N": "1"}}),
        )
        .unwrap()
    }

    #[test]
    fn get_set_remove() {
        let mut item = doc();
        assert_eq!(
            get(&item, &p(&[k("a"), k("b"), Elem::Index(1)])),
            Some(&Value::S("y".into()))
        );
        assert!(set(&mut item, &p(&[k("a"), k("c")]), Value::Null));
        assert!(set(
            &mut item,
            &p(&[k("a"), k("b"), Elem::Index(9)]),
            Value::S("z".into())
        ));
        assert!(!set(&mut item, &p(&[k("zz"), k("c")]), Value::Null));
        remove(&mut item, &p(&[k("a"), k("b"), Elem::Index(0)]));
        let Some(Value::L(l)) = get(&item, &p(&[k("a"), k("b")])) else {
            panic!("list")
        };
        assert_eq!(l, &[Value::S("y".into()), Value::S("z".into())]);
        remove(&mut item, &p(&[k("nothing"), k("here")]));
    }

    #[test]
    fn projection_keeps_the_shape() {
        let item = doc();
        let out = project(
            &item,
            &[
                p(&[k("a"), k("b"), Elem::Index(1)]),
                p(&[k("n")]),
                p(&[k("gone")]),
            ],
        );
        assert_eq!(out.len(), 2);
        assert_eq!(
            out.get("a"),
            Some(&Value::M(
                [("b".to_owned(), Value::L(vec![Value::S("y".into())]))].into()
            ))
        );
    }
}
