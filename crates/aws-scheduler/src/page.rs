//! Paging for the list operations, and timestamps as the wire carries them. A `NextToken` is the base64 of the key of the first item
//! of the next page, so it stays right while things are added or removed between calls (AWS's tokens are opaque; ours are too).

use aws_wire::ApiError;
use serde_json::Value;

use crate::req::{Req, invalid};

/// The most a page holds when the request names no `MaxResults` (the model allows 1 to 100).
const DEFAULT_LIMIT: usize = 100;

/// The page after the request's `NextToken`, from items in key order; and the token for the page after that, if there is one.
///
/// # Errors
/// `ValidationException` for a token that was not issued by this service.
pub(crate) fn page<T>(
    items: impl Iterator<Item = (String, T)>,
    r: &Req<'_>,
) -> Result<(Vec<T>, Option<String>), ApiError> {
    let after = match r.opt("NextToken") {
        Some(t) => Some(
            aws_core::base64::decode(t)
                .and_then(|b| String::from_utf8(b).ok())
                // (memory)
                .ok_or_else(|| invalid("Invalid NextToken."))?,
        ),
        None => None,
    };
    let limit = r
        .int("MaxResults")
        .and_then(|l| usize::try_from(l).ok())
        .filter(|l| *l > 0)
        .unwrap_or(DEFAULT_LIMIT);
    let mut rest = items.filter(|(k, _)| after.as_ref().is_none_or(|a| k.as_str() >= a.as_str()));
    let mut out: Vec<T> = Vec::new();
    for (key, item) in rest.by_ref() {
        if out.len() == limit {
            return Ok((out, Some(aws_core::base64::encode(key.as_bytes()))));
        }
        out.push(item);
    }
    Ok((out, None))
}

/// Adds a `NextToken` to an answer when there is one.
pub(crate) fn with_token(mut answer: Value, token: Option<String>) -> Value {
    if let (Some(t), Some(map)) = (token, answer.as_object_mut()) {
        map.insert("NextToken".to_owned(), Value::String(t));
    }
    answer
}

/// A timestamp as the wire carries it: seconds since the epoch, with milliseconds.
pub(crate) fn secs(ms: i64) -> Value {
    #[allow(clippy::cast_precision_loss)] // WHY: a time in ms is far below 2^53.
    Value::from(ms as f64 / 1000.0)
}
