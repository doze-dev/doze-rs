//! The errors Lambda raises, with the status and words AWS gave them (the recording of 2026-11-09 where there is one).

use aws_wire::ApiError;

pub(crate) fn not_found(what: impl Into<String>) -> ApiError {
    ApiError::new(404, "ResourceNotFoundException", what)
}

/// `Function not found: <arn>`.
pub(crate) fn no_function(arn: &str) -> ApiError {
    not_found(format!("Function not found: {arn}"))
}

pub(crate) fn conflict(what: impl Into<String>) -> ApiError {
    ApiError::new(409, "ResourceConflictException", what)
}

pub(crate) fn invalid(what: impl Into<String>) -> ApiError {
    ApiError::new(400, "InvalidParameterValueException", what)
}

pub(crate) fn invalid_content(what: impl Into<String>) -> ApiError {
    ApiError::new(400, "InvalidRequestContentException", what)
}

pub(crate) fn in_use(what: impl Into<String>) -> ApiError {
    ApiError::new(400, "ResourceInUseException", what)
}

pub(crate) fn precondition(what: impl Into<String>) -> ApiError {
    ApiError::new(412, "PreconditionFailedException", what)
}

pub(crate) fn throttled(reason: &str) -> ApiError {
    ApiError::new(429, "TooManyRequestsException", "Rate Exceeded.")
        .with("Reason", reason)
        .with("retryAfterSeconds", "1")
}

pub(crate) fn refused(op: &str) -> ApiError {
    ApiError::new(501, "NotImplemented", format!("{op} is not built in doze-aws."))
}

/// A function that cannot run here (a container image): stored and listed, and an invoke says why.
pub(crate) fn not_runnable(why: &str) -> ApiError {
    ApiError::new(502, "InvalidRuntimeException", why)
}
