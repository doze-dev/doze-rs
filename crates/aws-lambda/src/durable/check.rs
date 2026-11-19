//! The checks a checkpoint's updates must pass before any is applied. The model's own constraints (lengths, patterns, ranges,
//! enums) are checked by the front end; these are the service's, as AWS's checkpoint server (`@aws/durable-execution-sdk-js-testing`
//! 1.3.2, `validators/`) states them, with its words. All are `InvalidParameterValueException`.

use std::collections::HashMap;

use aws_wire::ApiError;
use serde_json::Value;

use super::Op;
use crate::errors::invalid;

/// The limit for a step's or context's output and for an error (checkpoint server: "Production uses 256 KB").
const CHECKPOINT_PAYLOAD: usize = 262_144;
/// The limit for a chained invoke's input.
const CHAINED_INPUT: usize = 1_048_576;
/// The limit for a synchronous execution's output (the model: 6 MB for `RequestResponse`, 1 MB for `Event`).
const SYNC_OUTPUT: usize = 6_291_456;
const ASYNC_OUTPUT: usize = 1_048_576;

const INCONSISTENT: &str =
    " value in OperationUpdate is inconsistent with existing operation. Check for non-determinism.";

pub(crate) fn s<'a>(u: &'a Value, k: &str) -> Option<&'a str> {
    u.get(k).and_then(Value::as_str)
}

fn has(u: &Value, k: &str) -> bool {
    u.get(k).is_some_and(|v| !v.is_null())
}

/// Checks every update of one request against the operations the execution has.
///
/// # Errors
/// The first rule an update breaks.
pub(crate) fn updates(updates: &[Value], ops: &[Op], sync: bool) -> Result<(), ApiError> {
    if updates.is_empty() {
        return Ok(());
    }
    let executions = updates
        .iter()
        .filter(|u| s(u, "Type") == Some("EXECUTION"))
        .count();
    if executions > 1 {
        return Err(invalid("Cannot checkpoint multiple EXECUTION updates."));
    }
    if executions == 1 && updates.last().and_then(|u| s(u, "Type")) != Some("EXECUTION") {
        return Err(invalid("EXECUTION checkpoint must be the last update."));
    }
    let by_id: HashMap<&str, &Op> = ops.iter().map(|o| (o.id.as_str(), o)).collect();
    parents_and_duplicates(updates, &by_id)?;
    for u in updates {
        let existing = s(u, "Id").and_then(|id| by_id.get(id).copied());
        one(u, existing, sync)?;
    }
    Ok(())
}

fn parents_and_duplicates(updates: &[Value], by_id: &HashMap<&str, &Op>) -> Result<(), ApiError> {
    let mut started: HashMap<&str, &str> = HashMap::new();
    let mut last: HashMap<&str, (&str, &str)> = HashMap::new();
    for u in updates {
        let (id, kind, action) = (
            s(u, "Id").unwrap_or_default(),
            s(u, "Type").unwrap_or_default(),
            s(u, "Action").unwrap_or_default(),
        );
        if let Some((kind_seen, action_seen)) = last.get(id) {
            let allowed = matches!(*kind_seen, "STEP" | "CONTEXT")
                && *action_seen == "START"
                && action != "START";
            if !allowed {
                return Err(invalid(
                    "Cannot update the same operation twice in a single request.",
                ));
            }
        }
        if let Some(parent) = s(u, "ParentId") {
            let parent_kind = started
                .get(parent)
                .copied()
                .or_else(|| by_id.get(parent).map(|o| o.kind.as_str()));
            if parent_kind != Some("CONTEXT") {
                return Err(invalid("Invalid parent operation id."));
            }
        }
        if action == "START" {
            started.insert(id, kind);
        }
        last.insert(id, (kind, action));
    }
    Ok(())
}

fn one(u: &Value, existing: Option<&Op>, sync: bool) -> Result<(), ApiError> {
    if let Some(op) = existing {
        let fields = [
            ("Type", Some(op.kind.as_str())),
            ("SubType", op.sub.as_deref()),
            ("Name", op.name.as_deref()),
            ("ParentId", op.parent.as_deref()),
        ];
        for (field, stored) in fields {
            if let Some(given) = s(u, field)
                && Some(given) != stored
            {
                return Err(invalid(format!("{field}{INCONSISTENT}")));
            }
        }
    }
    sizes(u, sync)?;
    actions(u)?;
    transition(u, existing)
}

fn sizes(u: &Value, sync: bool) -> Result<(), ApiError> {
    if has(u, "Error") && u.to_string().len() > CHECKPOINT_PAYLOAD {
        return Err(invalid(format!(
            "Error object size must be less than or equal to {CHECKPOINT_PAYLOAD} bytes."
        )));
    }
    let limit = match s(u, "Type") {
        Some("STEP") => Some(("STEP output payload", CHECKPOINT_PAYLOAD)),
        Some("CONTEXT") => Some(("CONTEXT output payload", CHECKPOINT_PAYLOAD)),
        Some("CHAINED_INVOKE") => Some(("CHAINED_INVOKE input payload", CHAINED_INPUT)),
        Some("EXECUTION") => Some((
            "Execution output payload",
            if sync { SYNC_OUTPUT } else { ASYNC_OUTPUT },
        )),
        _ => None,
    };
    if let (Some((what, max)), Some(p)) = (limit, s(u, "Payload"))
        && p.len() > max
    {
        return Err(invalid(format!(
            "{what} size must be less than or equal to {max} bytes."
        )));
    }
    Ok(())
}

fn actions(u: &Value) -> Result<(), ApiError> {
    let valid: &[&str] = match s(u, "Type") {
        Some("STEP") => &["START", "FAIL", "RETRY", "SUCCEED"],
        Some("CONTEXT") => &["START", "FAIL", "SUCCEED"],
        Some("WAIT") => &["START", "CANCEL"],
        Some("CALLBACK" | "CHAINED_INVOKE") => &["START"],
        Some("EXECUTION") => &["SUCCEED", "FAIL"],
        _ => return Err(invalid("Invalid operation type.")),
    };
    if s(u, "Action").is_some_and(|a| valid.contains(&a)) {
        Ok(())
    } else {
        Err(invalid("Invalid action for the given operation type."))
    }
}

fn closing(u: &Value) -> Result<(), ApiError> {
    match s(u, "Action") {
        Some("FAIL") if has(u, "Payload") => {
            Err(invalid("Cannot provide a Payload for FAIL action."))
        }
        Some("SUCCEED") if has(u, "Error") => {
            Err(invalid("Cannot provide an Error for SUCCEED action."))
        }
        _ => Ok(()),
    }
}

fn transition(u: &Value, op: Option<&Op>) -> Result<(), ApiError> {
    let status = op.map(|o| o.status.as_str());
    let action = s(u, "Action").unwrap_or_default();
    match s(u, "Type").unwrap_or_default() {
        "CALLBACK" if op.is_some() => Err(invalid(
            "Cannot start a CALLBACK operation that already exists.",
        )),
        "CHAINED_INVOKE" if op.is_some() => Err(invalid(
            "Cannot start a CHAINED_INVOKE operation that already exists.",
        )),
        "CHAINED_INVOKE" if !has(u, "ChainedInvokeOptions") => Err(invalid(
            "Update for CHAINED_INVOKE operation requires ChainedInvokeOptions.",
        )),
        "CONTEXT" => context(u, action, status),
        "EXECUTION" => closing(u),
        "STEP" => step(u, action, status),
        "WAIT" => wait(u, action, status),
        _ => Ok(()),
    }
}

fn context(u: &Value, action: &str, status: Option<&str>) -> Result<(), ApiError> {
    if action == "START" {
        return match status {
            Some(_) => Err(invalid(
                "Cannot start a CONTEXT operation that already exists.",
            )),
            None => Ok(()),
        };
    }
    if status.is_some_and(|st| st != "STARTED") {
        return Err(invalid("Invalid current CONTEXT state to close."));
    }
    closing(u)
}

fn step(u: &Value, action: &str, status: Option<&str>) -> Result<(), ApiError> {
    let within = |allowed: &[&str]| status.is_none_or(|st| allowed.contains(&st));
    match action {
        "START" if !within(&["READY"]) => Err(invalid("Invalid current STEP state to start.")),
        "FAIL" | "SUCCEED" if !within(&["STARTED", "READY"]) => {
            Err(invalid("Invalid current STEP state to close."))
        }
        "FAIL" | "SUCCEED" => closing(u),
        "RETRY" if !within(&["STARTED", "READY"]) => {
            Err(invalid("Invalid current STEP state to retry."))
        }
        "RETRY" if !has(u, "StepOptions") => Err(invalid(
            "RETRY action for STEP operation requires StepOptions.",
        )),
        "RETRY" if has(u, "Error") && has(u, "Payload") => Err(invalid(
            "Cannot provide both an Error and Payload for RETRY action.",
        )),
        _ => Ok(()),
    }
}

fn wait(u: &Value, action: &str, status: Option<&str>) -> Result<(), ApiError> {
    match action {
        "START" if status.is_some() => Err(invalid(
            "Cannot start a WAIT operation that already exists.",
        )),
        "START" if !has(u, "WaitOptions") => {
            Err(invalid("Update for WAIT operation requires WaitOptions."))
        }
        "CANCEL" if status != Some("STARTED") => Err(invalid(
            "Cannot cancel a WAIT operation that does not exist or has already completed.",
        )),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use serde_json::json;

    use super::*;

    fn op(id: &str, kind: &str, status: &str) -> Op {
        Op {
            id: id.into(),
            kind: kind.into(),
            status: status.into(),
            ..Op::default()
        }
    }

    fn message(u: &[Value], ops: &[Op]) -> String {
        updates(u, ops, true).unwrap_err().message
    }

    #[test]
    fn the_execution_update_comes_last_and_once() {
        let exec = json!({"Id": "e", "Type": "EXECUTION", "Action": "SUCCEED"});
        let step = json!({"Id": "s", "Type": "STEP", "Action": "START"});
        assert_eq!(
            message(&[exec.clone(), step.clone()], &[]),
            "EXECUTION checkpoint must be the last update."
        );
        assert_eq!(
            message(&[exec.clone(), exec.clone()], &[]),
            "Cannot checkpoint multiple EXECUTION updates."
        );
        assert!(updates(&[step, exec], &[], true).is_ok());
    }

    #[test]
    fn a_step_may_start_and_close_in_one_request_but_not_twice() {
        let start = json!({"Id": "s", "Type": "STEP", "Action": "START"});
        let done = json!({"Id": "s", "Type": "STEP", "Action": "SUCCEED", "Payload": "1"});
        assert!(updates(&[start.clone(), done.clone()], &[], true).is_ok());
        assert_eq!(
            message(&[done.clone(), done], &[]),
            "Cannot update the same operation twice in a single request."
        );
        assert_eq!(
            message(&[start], &[op("s", "STEP", "SUCCEEDED")]),
            "Invalid current STEP state to start."
        );
    }

    #[test]
    fn a_parent_must_be_a_context_and_metadata_must_not_change() {
        let child = json!({"Id": "c", "ParentId": "w", "Type": "STEP", "Action": "START"});
        assert_eq!(
            message(&[child], &[op("w", "WAIT", "STARTED")]),
            "Invalid parent operation id."
        );
        let renamed = json!({"Id": "s", "Name": "other", "Type": "STEP", "Action": "SUCCEED"});
        assert_eq!(
            message(&[renamed], &[op("s", "STEP", "STARTED")]),
            "Name value in OperationUpdate is inconsistent with existing operation. Check for non-determinism."
        );
        let cb = json!({"Id": "b", "Type": "CALLBACK", "Action": "SUCCEED"});
        assert_eq!(
            message(&[cb], &[]),
            "Invalid action for the given operation type."
        );
    }
}
