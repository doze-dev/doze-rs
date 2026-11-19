//! Keys in dot notation (`store.book.title`): reading, writing and removing a member of the event's JSON object.

use serde_json::{Map, Value};

/// How deep a path may go.
const MAX_DEPTH: usize = 32;

fn parts(path: &str) -> Option<Vec<&str>> {
    let parts: Vec<&str> = path.split('.').collect();
    (!path.is_empty() && parts.len() <= MAX_DEPTH && parts.iter().all(|p| !p.is_empty()))
        .then_some(parts)
}

/// The value at `path`.
pub(crate) fn get<'a>(root: &'a Map<String, Value>, path: &str) -> Option<&'a Value> {
    let parts = parts(path)?;
    let (last, parents) = parts.split_last()?;
    let mut at = root;
    for p in parents {
        at = at.get(*p)?.as_object()?;
    }
    at.get(*last)
}

/// The object that holds `path`'s last part, made on the way when `make`; `None` when a part on the way is not an object.
fn parent<'a, 'p>(
    root: &'a mut Map<String, Value>,
    path: &'p str,
    make: bool,
) -> Option<(&'a mut Map<String, Value>, &'p str)> {
    let parts = parts(path)?;
    let (last, parents) = parts.split_last()?;
    let mut at = root;
    for p in parents {
        if make && !at.contains_key(*p) {
            at.insert((*p).to_owned(), Value::Object(Map::new()));
        }
        at = at.get_mut(*p)?.as_object_mut()?;
    }
    Some((at, last))
}

/// Whether `path` holds a value.
pub(crate) fn has(root: &Map<String, Value>, path: &str) -> bool {
    get(root, path).is_some()
}

/// Sets `path` to `value`, making the objects on the way; `false` when a part on the way is not an object.
pub(crate) fn set(root: &mut Map<String, Value>, path: &str, value: Value) -> bool {
    match parent(root, path, true) {
        Some((obj, last)) => {
            obj.insert(last.to_owned(), value);
            true
        }
        None => false,
    }
}

/// Sets `path` unless it holds a value and `overwrite` is false.
pub(crate) fn put(root: &mut Map<String, Value>, path: &str, value: Value, overwrite: bool) {
    if overwrite || !has(root, path) {
        set(root, path, value);
    }
}

/// Removes and returns the value at `path`.
pub(crate) fn take(root: &mut Map<String, Value>, path: &str) -> Option<Value> {
    let (obj, last) = parent(root, path, false)?;
    obj.remove(last)
}

/// The value at `path`, to change in place.
pub(crate) fn get_mut<'a>(root: &'a mut Map<String, Value>, path: &str) -> Option<&'a mut Value> {
    let (obj, last) = parent(root, path, false)?;
    obj.get_mut(last)
}
