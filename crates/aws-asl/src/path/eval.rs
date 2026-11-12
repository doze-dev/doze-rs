//! Reading a path against a document, and writing a value at a reference path.

use serde_json::{Map, Value};

use super::{Path, Root, Scope, Seg};

pub(super) fn select(path: &Path, scope: Scope<'_>) -> Option<Value> {
    let start = match &path.root {
        Root::Input => scope.data,
        Root::Context => scope.context,
        Root::Var(name) => scope.vars.get(name)?,
    };
    let mut nodes: Vec<Value> = vec![start.clone()];
    for seg in &path.segs {
        let mut next = Vec::new();
        for node in &nodes {
            step(seg, node, scope, &mut next);
        }
        nodes = next;
    }
    if path.definite() {
        nodes.into_iter().next()
    } else {
        Some(Value::Array(nodes))
    }
}

/// The index `i` of an array of `len`, counting from the end when negative.
fn at(i: i64, len: usize) -> Option<usize> {
    let len = i64::try_from(len).ok()?;
    let i = if i < 0 { len + i } else { i };
    if (0..len).contains(&i) {
        usize::try_from(i).ok()
    } else {
        None
    }
}

fn step(seg: &Seg, node: &Value, scope: Scope<'_>, out: &mut Vec<Value>) {
    match seg {
        Seg::Field(name) => out.extend(node.get(name).cloned()),
        Seg::Index(i) => {
            if let Value::Array(a) = node {
                out.extend(at(*i, a.len()).and_then(|i| a.get(i)).cloned());
            }
        }
        Seg::Wild => match node {
            Value::Array(a) => out.extend(a.iter().cloned()),
            Value::Object(o) => out.extend(o.values().cloned()),
            _ => {}
        },
        Seg::Fields(names) => {
            for n in names {
                out.extend(node.get(n).cloned());
            }
        }
        Seg::Indices(ix) => {
            if let Value::Array(a) = node {
                for i in ix {
                    out.extend(at(*i, a.len()).and_then(|i| a.get(i)).cloned());
                }
            }
        }
        Seg::Slice(from, to, by) => {
            if let Value::Array(a) = node {
                slice(a, *from, *to, *by, out);
            }
        }
        Seg::Descend(inner) => descend(inner, node, scope, out),
        Seg::Filter(f) => match node {
            Value::Array(a) => out.extend(a.iter().filter(|v| f.holds(v, scope)).cloned()),
            Value::Object(_) if f.holds(node, scope) => out.push(node.clone()),
            _ => {}
        },
        Seg::Length => match node {
            Value::Array(a) => out.push(Value::from(a.len())),
            Value::Object(o) => out.push(Value::from(o.len())),
            Value::String(s) => out.push(Value::from(s.chars().count())),
            _ => {}
        },
    }
}

fn slice(a: &[Value], from: Option<i64>, to: Option<i64>, by: Option<i64>, out: &mut Vec<Value>) {
    let len = i64::try_from(a.len()).unwrap_or(i64::MAX);
    let fix = |i: i64| if i < 0 { (len + i).max(0) } else { i.min(len) };
    let (start, end) = (fix(from.unwrap_or(0)), fix(to.unwrap_or(len)));
    let by = by.unwrap_or(1).max(1);
    let mut i = start;
    while i < end {
        if let Some(v) = usize::try_from(i).ok().and_then(|i| a.get(i)) {
            out.push(v.clone());
        }
        i += by;
    }
}

fn descend(inner: &Seg, node: &Value, scope: Scope<'_>, out: &mut Vec<Value>) {
    step(inner, node, scope, out);
    match node {
        Value::Array(a) => a.iter().for_each(|v| descend(inner, v, scope, out)),
        Value::Object(o) => o.values().for_each(|v| descend(inner, v, scope, out)),
        _ => {}
    }
}

/// A reference path that cannot be followed in a document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unreachable;

/// Writes `value` into `doc` at the reference path, making objects on the way. A step that meets the wrong kind of node (a field of an
/// array, an index past the end) is `Err` with the step's position.
///
/// # Errors
/// The path cannot be followed in `doc`.
pub fn set_at(doc: &mut Value, path: &Path, value: Value) -> Result<(), Unreachable> {
    let mut cur = doc;
    for seg in &path.segs {
        cur = match seg {
            Seg::Field(name) => {
                if cur.is_null() {
                    *cur = Value::Object(Map::new());
                }
                let Value::Object(o) = cur else {
                    return Err(Unreachable);
                };
                o.entry(name.clone()).or_insert(Value::Null)
            }
            Seg::Index(i) => {
                let Value::Array(a) = cur else {
                    return Err(Unreachable);
                };
                let i = at(*i, a.len()).ok_or(Unreachable)?;
                a.get_mut(i).ok_or(Unreachable)?
            }
            _ => return Err(Unreachable),
        };
    }
    *cur = value;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::{Path, Scope};
    use super::set_at;

    fn sel(p: &str, data: &serde_json::Value) -> Option<serde_json::Value> {
        let none = json!({});
        Path::parse(p).unwrap().select(Scope {
            data,
            context: &none,
            vars: &none,
        })
    }

    #[test]
    fn selection() {
        let d =
            json!({"a": [{"x": 1, "y": "p"}, {"x": 5}, {"x": 9, "y": "q"}], "b": {"c": {"x": 2}}});
        assert_eq!(sel("$.a[0].x", &d), Some(json!(1)));
        assert_eq!(sel("$.a[-1].x", &d), Some(json!(9)));
        assert_eq!(sel("$.a[*].x", &d), Some(json!([1, 5, 9])));
        assert_eq!(sel("$.a[*].y", &d), Some(json!(["p", "q"])));
        assert_eq!(sel("$.a[0:2].x", &d), Some(json!([1, 5])));
        assert_eq!(sel("$..x", &d), Some(json!([1, 5, 9, 2])));
        assert_eq!(sel("$.a[?(@.x > 2)].x", &d), Some(json!([5, 9])));
        assert_eq!(sel("$.a[?(@.y)].x", &d), Some(json!([1, 9])));
        assert_eq!(sel("$.a[?(@.y == 'q')].x", &d), Some(json!([9])));
        assert_eq!(sel("$.a.length()", &d), Some(json!(3)));
        assert_eq!(sel("$.nope", &d), None);
        assert_eq!(sel("$.nope[*]", &d), Some(json!([])));
    }

    #[test]
    fn writing() {
        let mut d = json!({"kept": 1});
        set_at(&mut d, &Path::parse("$.a.b").unwrap(), json!(true)).unwrap();
        assert_eq!(d, json!({"kept": 1, "a": {"b": true}}));
        assert!(set_at(&mut d, &Path::parse("$.kept.x").unwrap(), json!(1)).is_err());
        let mut arr = json!([1, 2]);
        set_at(&mut arr, &Path::parse("$[1]").unwrap(), json!(3)).unwrap();
        assert_eq!(arr, json!([1, 3]));
    }
}
