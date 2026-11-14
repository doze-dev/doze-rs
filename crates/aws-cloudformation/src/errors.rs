//! The errors CloudFormation answers with. Its Query front end spells almost every refusal `ValidationError` (400), state errors
//! included; the few modelled ones (`AlreadyExistsException`, `ChangeSetNotFound`, ...) carry their own code.

use aws_wire::ApiError;

/// `ValidationError` with `message`.
pub(crate) fn validation(message: impl Into<String>) -> ApiError {
    ApiError::new(400, "ValidationError", message)
}

/// `Template format error: <what>`.
pub(crate) fn format(what: impl AsRef<str>) -> ApiError {
    validation(format!("Template format error: {}", what.as_ref()))
}

/// `Template error: <what>`.
pub(crate) fn template(what: impl AsRef<str>) -> ApiError {
    validation(format!("Template error: {}", what.as_ref()))
}

/// `Stack with id <name> does not exist` (recorded 2026-11-12, for DescribeStacks and ListStackResources).
pub(crate) fn no_stack(name: &str) -> ApiError {
    validation(format!("Stack with id {name} does not exist"))
}

/// `Stack [<name>] does not exist` (recorded 2026-11-12, for UpdateStack).
pub(crate) fn no_stack_bracketed(name: &str) -> ApiError {
    validation(format!("Stack [{name}] does not exist"))
}

/// `Stack [<name>] already exists` (recorded 2026-11-12).
pub(crate) fn exists(name: &str) -> ApiError {
    ApiError::modeled(
        "AlreadyExistsException",
        format!("Stack [{name}] already exists"),
    )
}

/// `ChangeSet [<name>] does not exist` (recorded 2026-11-12: 404).
pub(crate) fn no_change_set(name: &str) -> ApiError {
    ApiError::modeled(
        "ChangeSetNotFoundException",
        format!("ChangeSet [{name}] does not exist"),
    )
}

/// The capabilities a template needs and the call did not acknowledge. (memory) AWS prints a space before the colon.
pub(crate) fn capabilities(needed: &[&str]) -> ApiError {
    ApiError::modeled(
        "InsufficientCapabilitiesException",
        format!("Requires capabilities : [{}]", needed.join(", ")),
    )
}

/// A refusal of an operation in flight: `Stack:<id> is in <status> state and can not be updated.` (recorded 2026-11-12).
pub(crate) fn cannot_update(stack_id: &str, status: &str) -> ApiError {
    validation(format!(
        "Stack:{stack_id} is in {status} state and can not be updated."
    ))
}
