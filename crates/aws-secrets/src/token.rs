//! Paging by position: a token says how many entries came before the page it starts.

use aws_wire::ApiError;

/// The token for a page that starts after `n` entries.
pub(crate) fn of(n: usize) -> String {
    aws_core::hex::encode(n.to_string().as_bytes())
}

/// How many entries a token says came before its page.
pub(crate) fn resume(token: &str) -> Result<usize, ApiError> {
    aws_core::hex::decode(token)
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| {
            ApiError::modeled(
                "InvalidNextTokenException",
                "The NextToken value is not valid.",
            )
        })
}
