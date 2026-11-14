//! The checks on a schedule's members that the model does not make. The sentences are from memory of AWS's answers.

use aws_wire::ApiError;
use serde_json::Value;

use crate::req::invalid;

/// `StartDate` and `EndDate` of a recurring schedule (a one-time schedule ignores both, as documented).
pub(crate) fn dates(start: Option<i64>, end: Option<i64>, now: i64) -> Result<(), ApiError> {
    if let Some(end) = end {
        if end <= now {
            // (memory)
            return Err(invalid("The EndDate must be in the future."));
        }
        if start.is_some_and(|s| s >= end) {
            // (memory)
            return Err(invalid("The StartDate must come before the EndDate."));
        }
    }
    Ok(())
}

/// `FlexibleTimeWindow`: `OFF` takes no window, `FLEXIBLE` needs one.
pub(crate) fn window(w: &Value) -> Result<(), ApiError> {
    let mode = w.get("Mode").and_then(Value::as_str).unwrap_or_default();
    let minutes = w.get("MaximumWindowInMinutes");
    match (mode, minutes) {
        // (memory)
        ("FLEXIBLE", None) => Err(invalid(
            "MaximumWindowInMinutes must be provided when the FlexibleTimeWindow mode is FLEXIBLE.",
        )),
        // (memory)
        ("OFF", Some(_)) => Err(invalid(
            "MaximumWindowInMinutes must not be provided when the FlexibleTimeWindow mode is OFF.",
        )),
        _ => Ok(()),
    }
}
