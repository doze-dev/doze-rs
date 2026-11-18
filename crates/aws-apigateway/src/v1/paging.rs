//! Paging of v1 lists: `limit` (25 when not given, at most 500) and `position`, which here is the offset of the next item as text
//! (AWS's is opaque; a client only hands it back).

use serde_json::{Value, json};

use crate::req::Req;

/// One page of `items`, as `{item: [...], position}`.
pub(crate) fn page(items: Vec<Value>, r: &Req) -> Value {
    let limit = r
        .int("limit")
        .and_then(|l| usize::try_from(l).ok())
        .filter(|l| *l > 0)
        .unwrap_or(25)
        .min(500);
    let from = r
        .opt("position")
        .and_then(|p| p.parse::<usize>().ok())
        .unwrap_or(0);
    let total = items.len();
    let shown: Vec<Value> = items.into_iter().skip(from).take(limit).collect();
    let next = from + shown.len();
    let mut out = json!({"item": shown});
    if next < total {
        crate::doc::put(&mut out, "position", Value::String(next.to_string()));
    }
    out
}
