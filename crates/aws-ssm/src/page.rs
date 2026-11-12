//! Paging: a token is the name to continue after.

use aws_wire::ApiError;

/// The token for a page that ended at `last`.
pub(crate) fn token(last: &str) -> String {
    aws_core::hex::encode(last.as_bytes())
}

/// The name a token continues after.
pub(crate) fn resume(token: &str) -> Result<String, ApiError> {
    let bytes = aws_core::hex::decode(token).ok_or_else(invalid_token)?;
    String::from_utf8(bytes).map_err(|_| invalid_token())
}

fn invalid_token() -> ApiError {
    ApiError::modeled("InvalidNextToken", "The specified token is not valid.")
}
