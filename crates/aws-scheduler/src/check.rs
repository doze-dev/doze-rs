//! The checks on a schedule's members that the model does not make. The sentences marked `(memory)` are from memory of AWS's
//! answers; the others were recorded 2026-11-15.

use aws_wire::ApiError;
use serde_json::Value;

use crate::req::invalid;

/// `StartDate` and `EndDate` of a recurring schedule (a one-time schedule ignores both, as documented).
pub(crate) fn dates(start: Option<i64>, end: Option<i64>, now: i64) -> Result<(), ApiError> {
    if let Some(end) = end {
        // A schedule with no StartDate starts now, so an EndDate in the past is this refusal (recorded).
        if end <= start.unwrap_or(now) {
            return Err(invalid(
                "The StartDate you specify must come before the EndDate.",
            ));
        }
        if end <= now {
            // (memory)
            return Err(invalid("The EndDate must be in the future."));
        }
    }
    Ok(())
}

/// `FlexibleTimeWindow`: `OFF` takes no window, `FLEXIBLE` needs one.
pub(crate) fn window(w: &Value) -> Result<(), ApiError> {
    let mode = w.get("Mode").and_then(Value::as_str).unwrap_or_default();
    let minutes = w.get("MaximumWindowInMinutes");
    match (mode, minutes) {
        ("FLEXIBLE", None) => Err(invalid(
            "MaximumWindowInMinutes must be provided when FlexibleTimeWindowMode is set to FLEXIBLE.",
        )),
        // (memory)
        ("OFF", Some(_)) => Err(invalid(
            "MaximumWindowInMinutes must not be provided when the FlexibleTimeWindow mode is OFF.",
        )),
        _ => Ok(()),
    }
}
