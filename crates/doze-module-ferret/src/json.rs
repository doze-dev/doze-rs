//! Seed files: a JSON array of documents, read into BSON with each number keeping its kind. Go decoded seeds into
//! `bson.M` through `encoding/json`, so every integer became a double (`{"qty": 3}` was stored as `3.0`; modules plan
//! bug 6). Here an integer stays an integer: int32 when it fits, int64 when it needs to, a double only when the text
//! has a fraction or an exponent, or the integer is beyond int64.

use crate::bson::{Bson, Doc};
use serde_json::Value;

/// The documents of a seed file, or why it is not one.
pub(crate) fn documents(text: &str) -> Result<Vec<Doc>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("it is not JSON ({e})"))?;
    let Value::Array(items) = v else {
        return Err("it must be a JSON array of documents".into());
    };
    if let Some(k) = nul_key(&Value::Array(items.clone())) {
        return Err(format!(
            "the key {k:?} holds a NUL, which BSON cannot store"
        ));
    }
    items
        .into_iter()
        .enumerate()
        .map(|(i, item)| match item {
            Value::Object(o) => Ok(object(o)),
            _ => Err(format!("item {i} is not a document (an object)")),
        })
        .collect()
}

/// A key anywhere in `v` that holds a NUL (BSON keys end at one).
fn nul_key(v: &Value) -> Option<String> {
    match v {
        Value::Object(o) => o.iter().find_map(|(k, v)| {
            if k.contains('\0') {
                Some(k.clone())
            } else {
                nul_key(v)
            }
        }),
        Value::Array(a) => a.iter().find_map(nul_key),
        _ => None,
    }
}

fn object(o: serde_json::Map<String, Value>) -> Doc {
    Doc(o.into_iter().map(|(k, v)| (k, value(v))).collect())
}

fn value(v: Value) -> Bson {
    match v {
        Value::Null => Bson::Null,
        Value::Bool(b) => Bson::Bool(b),
        Value::Number(n) => number(&n),
        Value::String(s) => Bson::String(s),
        Value::Array(a) => Bson::Array(a.into_iter().map(value).collect()),
        Value::Object(o) => Bson::Doc(object(o)),
    }
}

fn number(n: &serde_json::Number) -> Bson {
    if let Some(i) = n.as_i64() {
        return i32::try_from(i).map_or(Bson::Int64(i), Bson::Int32);
    }
    Bson::Double(n.as_f64().unwrap_or(f64::NAN))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn integers_stay_integers() {
        let d = documents(
            r#"[{"a": 3, "b": 3.0, "c": 5000000000, "d": 18446744073709551615, "e": -1}]"#,
        )
        .unwrap();
        let d = &d[0];
        assert_eq!(d.get("a"), Some(&Bson::Int32(3)));
        assert_eq!(d.get("b"), Some(&Bson::Double(3.0)));
        assert_eq!(d.get("c"), Some(&Bson::Int64(5_000_000_000)));
        assert_eq!(
            d.get("d"),
            Some(&Bson::Double(18_446_744_073_709_551_615.0))
        );
        assert_eq!(d.get("e"), Some(&Bson::Int32(-1)));
    }

    #[test]
    fn fields_come_back_sorted_as_go_stored_them() {
        // serde_json's preserve_order is not on (it would change every crate's JSON in a workspace build): keys come
        // back sorted, as Go's map stored them.
        let d = documents(r#"[{"_id": 1, "tags": ["x", {"y": null}], "ok": true}]"#).unwrap();
        let keys: Vec<&str> = d[0].0.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["_id", "ok", "tags"]);
    }

    #[test]
    fn what_is_not_an_array_of_documents_says_so() {
        assert!(documents("{}").unwrap_err().contains("array"));
        assert!(documents("[1]").unwrap_err().contains("item 0"));
        assert!(documents("[").unwrap_err().contains("not JSON"));
        assert!(
            documents(r#"[{"a": {"b\u0000c": 1}}]"#)
                .unwrap_err()
                .contains("NUL")
        );
    }
}
