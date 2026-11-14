//! The error names the States Language reserves, and a failure as an execution sees one: a name that `Retry` and `Catch` match, and
//! a cause a person reads.

use serde::{Deserialize, Serialize};

/// Matches every error. Allowed alone, in the last retrier or catcher.
pub const ALL: &str = "States.ALL";
pub const BRANCH_FAILED: &str = "States.BranchFailed";
pub const DATA_LIMIT_EXCEEDED: &str = "States.DataLimitExceeded";
pub const EXCEED_TOLERATED_FAILURE_THRESHOLD: &str = "States.ExceedToleratedFailureThreshold";
pub const HEARTBEAT_TIMEOUT: &str = "States.HeartbeatTimeout";
pub const INTRINSIC_FAILURE: &str = "States.IntrinsicFailure";
pub const ITEM_READER_FAILED: &str = "States.ItemReaderFailed";
pub const NO_CHOICE_MATCHED: &str = "States.NoChoiceMatched";
pub const PARAMETER_PATH_FAILURE: &str = "States.ParameterPathFailure";
pub const PERMISSIONS: &str = "States.Permissions";
pub const QUERY_EVALUATION_ERROR: &str = "States.QueryEvaluationError";
pub const RESULT_PATH_MATCH_FAILURE: &str = "States.ResultPathMatchFailure";
pub const RESULT_WRITER_FAILED: &str = "States.ResultWriterFailed";
pub const RUNTIME: &str = "States.Runtime";
pub const TASK_FAILED: &str = "States.TaskFailed";
pub const TIMEOUT: &str = "States.Timeout";

/// Every name above. A user's own error may not start with `States.` unless it is one of these.
pub const RESERVED: &[&str] = &[
    ALL,
    BRANCH_FAILED,
    DATA_LIMIT_EXCEEDED,
    EXCEED_TOLERATED_FAILURE_THRESHOLD,
    HEARTBEAT_TIMEOUT,
    INTRINSIC_FAILURE,
    ITEM_READER_FAILED,
    NO_CHOICE_MATCHED,
    PARAMETER_PATH_FAILURE,
    PERMISSIONS,
    QUERY_EVALUATION_ERROR,
    RESULT_PATH_MATCH_FAILURE,
    RESULT_WRITER_FAILED,
    RUNTIME,
    TASK_FAILED,
    TIMEOUT,
];

/// An error name and its cause.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    pub error: String,
    pub cause: String,
}

impl Failure {
    #[must_use]
    pub fn new(error: impl Into<String>, cause: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            cause: cause.into(),
        }
    }

    /// `States.Runtime` with `cause`.
    #[must_use]
    pub fn runtime(cause: impl Into<String>) -> Self {
        Self::new(RUNTIME, cause)
    }
}

/// Whether `name` is matched by an `ErrorEquals` list. `States.ALL` matches every error but the terminal ones (`States.Runtime`,
/// `States.DataLimitExceeded`); `States.TaskFailed` matches every error but those and `States.Timeout` (AWS's documentation of the
/// error names). `States.Runtime` is never retried or caught, whatever the list says.
#[must_use]
pub fn matches(name: &str, equals: &[String]) -> bool {
    if name == RUNTIME {
        return false;
    }
    let terminal = name == DATA_LIMIT_EXCEEDED;
    equals.iter().any(|e| {
        e == name
            || (!terminal && e == ALL)
            || (!terminal && e == TASK_FAILED && name != TIMEOUT && !name.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_failed_matches_all_but_the_timeout() {
        let tf = vec![TASK_FAILED.to_owned()];
        assert!(matches("Lambda.ServiceException", &tf));
        assert!(matches(HEARTBEAT_TIMEOUT, &tf));
        assert!(!matches(TIMEOUT, &tf));
        assert!(matches(TIMEOUT, &[ALL.to_owned()]));
        assert!(!matches("A", &["B".to_owned()]));
    }
}
