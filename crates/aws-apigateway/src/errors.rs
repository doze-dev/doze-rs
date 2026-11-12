//! The errors API Gateway answers, with the codes and statuses of both APIs (the same four for v1 and v2). The words are AWS's where a
//! recording has them (2026-11-09: `test_apigateway.py`); the others are marked `(memory)` where they are written.

use aws_wire::ApiError;

pub(crate) fn not_found(what: impl Into<String>) -> ApiError {
    ApiError::new(404, "NotFoundException", what)
}

pub(crate) fn bad(what: impl Into<String>) -> ApiError {
    ApiError::new(400, "BadRequestException", what)
}

pub(crate) fn conflict(what: impl Into<String>) -> ApiError {
    ApiError::new(409, "ConflictException", what)
}

/// Recorded: `Invalid API identifier specified <account>:<id>` (v1 and v2 alike).
pub(crate) fn no_api(account: &str, id: &str) -> ApiError {
    not_found(format!("Invalid API identifier specified {account}:{id}"))
}

/// Recorded for a resource, a method, a stage and a deployment: `Invalid <Thing> identifier specified`.
pub(crate) fn invalid_id(thing: &str) -> ApiError {
    not_found(format!("Invalid {thing} identifier specified"))
}

/// An operation doze-aws refuses by name.
pub(crate) fn refused(op: &str) -> ApiError {
    ApiError::new(
        501,
        "NotImplemented",
        format!("{op} is not built in doze-aws."),
    )
}

/// v2 words a missing thing as `Invalid <Thing> identifier specified <account>:<id>`? Only the API is recorded; the others name the
/// thing and the id `(memory)`.
pub(crate) fn v2_missing(thing: &str, id: &str) -> ApiError {
    not_found(format!("Unable to find {thing} with id '{id}'"))
}
