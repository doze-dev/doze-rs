//! The JSONPath EventBridge accepts in `InputPath` and `InputPathsMap`: `$`, then fields by dot (`.detail.state`) or bracket
//! (`['detail']`, `["detail-type"]`), and array positions (`[0]`). No wildcards, slices or filters.

use serde_json::Value;

/// One step of a path.
#[derive(Debug, PartialEq, Eq)]
enum Step<'a> {
    Field(&'a str),
    Index(usize),
}

/// Whether `path` is one this module can follow.
pub(crate) fn is_valid(path: &str) -> bool {
    steps(path).is_some()
}

/// The value at `path` in `doc`, if there is one.
pub(crate) fn select(doc: &Value, path: &str) -> Option<Value> {
    let mut at = doc;
    for step in steps(path)? {
        at = match step {
            Step::Field(f) => at.get(f)?,
            Step::Index(i) => at.get(i)?,
        };
    }
    Some(at.clone())
}

fn steps(path: &str) -> Option<Vec<Step<'_>>> {
    let mut rest = path.strip_prefix('$')?;
    let mut out = Vec::new();
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('.') {
            let end = after.find(['.', '[']).unwrap_or(after.len());
            let (field, tail) = after.split_at(end);
            if field.is_empty() || field.contains('*') {
                return None;
            }
            out.push(Step::Field(field));
            rest = tail;
        } else {
            let (inside, tail) = rest.strip_prefix('[')?.split_once(']')?;
            out.push(bracket(inside)?);
            rest = tail;
        }
    }
    Some(out)
}

/// What is inside `[...]`: a quoted field or a number.
fn bracket(inside: &str) -> Option<Step<'_>> {
    for quote in ['\'', '"'] {
        if let Some(name) = inside
            .strip_prefix(quote)
            .and_then(|s| s.strip_suffix(quote))
        {
            return Some(Step::Field(name));
        }
    }
    inside.parse().ok().map(Step::Index)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn paths_by_dot_and_bracket() {
        let doc =
            json!({"detail": {"items": [{"id": 7}, {"id": 8}], "a-b": "x"}, "detail-type": "t"});
        assert_eq!(select(&doc, "$.detail.items[1].id"), Some(json!(8)));
        assert_eq!(select(&doc, "$['detail']['a-b']"), Some(json!("x")));
        assert_eq!(select(&doc, "$[\"detail-type\"]"), Some(json!("t")));
        assert_eq!(select(&doc, "$.detail-type"), Some(json!("t")));
        assert_eq!(select(&doc, "$"), Some(doc.clone()));
        assert_eq!(select(&doc, "$.detail.missing"), None);
        assert_eq!(select(&doc, "$.detail.items[5]"), None);
        for bad in ["detail", "$.", "$.a.*", "$..a", "$[", "$[x]", "$.a b[", ""] {
            assert!(!is_valid(bad), "{bad:?}");
        }
    }
}
