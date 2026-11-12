//! Paging for the list operations. A `NextToken` is the base64 of the key of the first item of the next page, so it stays right while
//! things are added or removed between calls. (AWS's tokens are opaque; ours are too.)

use aws_wire::ApiError;
use serde_json::Value;

use crate::req::{Req, invalid};

/// The most a page holds when the request names no `Limit` (AWS lists 100 at most; recorded for none of the list operations).
const DEFAULT_LIMIT: usize = 100;

/// The page after the request's `NextToken`, from items in key order; and the token for the page after that, if there is one.
///
/// # Errors
/// `ValidationException` for a token that was not issued by this service.
pub(crate) fn page<T>(
    items: impl Iterator<Item = (String, T)>,
    r: &Req<'_>,
    limit_member: &str,
) -> Result<(Vec<T>, Option<String>), ApiError> {
    let after = match r.opt("NextToken") {
        Some(t) => Some(
            aws_core::base64::decode(t)
                .and_then(|b| String::from_utf8(b).ok())
                .ok_or_else(|| invalid("Invalid token"))?,
        ),
        None => None,
    };
    let limit = r
        .int(limit_member)
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
