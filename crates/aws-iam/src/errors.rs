//! IAM's errors, and the words it gave when they were recorded (2026-11-09) or, where marked, from memory.

use aws_wire::ApiError;

pub(crate) fn no_such(what: &str, name: &str) -> ApiError {
    ApiError::modeled(
        "NoSuchEntityException",
        format!("The {what} with name {name} cannot be found."),
    )
}

pub(crate) fn no_such_policy(arn: &str) -> ApiError {
    ApiError::modeled(
        "NoSuchEntityException",
        format!("Policy {arn} was not found."),
    )
}

pub(crate) fn exists(what: &str, name: &str) -> ApiError {
    ApiError::modeled(
        "EntityAlreadyExistsException",
        format!("{what} with name {name} already exists."),
    )
}

pub(crate) fn conflict(message: &str) -> ApiError {
    ApiError::modeled("DeleteConflictException", message)
}

pub(crate) fn limit(message: impl Into<String>) -> ApiError {
    ApiError::modeled("LimitExceededException", message)
}

pub(crate) fn malformed(message: impl Into<String>) -> ApiError {
    ApiError::modeled("MalformedPolicyDocumentException", message)
}

pub(crate) fn invalid_input(message: impl Into<String>) -> ApiError {
    ApiError::modeled("InvalidInputException", message)
}

pub(crate) fn no_such_member(user: &str, group: &str) -> ApiError {
    // (memory)
    ApiError::modeled(
        "NoSuchEntityException",
        format!("User {user} is not a member of group {group}."),
    )
}
