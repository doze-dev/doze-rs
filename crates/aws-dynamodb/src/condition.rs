//! The parts of a request that are shared by the item operations: the condition, the projection, what to return, the capacity report.
//!
//! Wording: the messages in `conformance/snapshots/test_dynamodb.json` (recorded 2026-11-09) are AWS's own; every other message in this file is
//! `(memory)`: from the developer guide and from use, and unchecked until a recording reaches it.

use aws_expr::{
    Cond, Item, Kind, Path, Scope, item_from_json, item_to_json, parse_condition, parse_projection,
    project,
};
use aws_wire::ApiError;
use serde_json::{Value as Json, json};

use crate::req::{bad_param, inv, str_of};

/// Reads the `ConditionExpression` of a request.
pub(crate) fn condition(body: &Json, scope: &mut Scope<'_>) -> Result<Option<Cond>, ApiError> {
    match str_of(body, "ConditionExpression") {
        Some(text) => Ok(Some(
            parse_condition(text, Kind::Condition, scope).map_err(inv)?,
        )),
        None => Ok(None),
    }
}

/// Reads the `ProjectionExpression` of a request.
pub(crate) fn projection(
    body: &Json,
    scope: &mut Scope<'_>,
) -> Result<Option<Vec<Path>>, ApiError> {
    match str_of(body, "ProjectionExpression") {
        Some(text) => Ok(Some(parse_projection(text, scope).map_err(inv)?)),
        None => Ok(None),
    }
}

/// The names of the expressions a request used, for the unused-names check.
pub(crate) fn used<'a>(body: &Json, members: &[&'a str]) -> Vec<&'a str> {
    members
        .iter()
        .copied()
        .filter(|m| body.get(*m).is_some())
        .collect()
}

/// The refusal for a condition that did not hold, carrying the item when the request asked for it.
pub(crate) fn failed(body: &Json, old: Option<&Item>) -> ApiError {
    let e = ApiError::modeled(
        "ConditionalCheckFailedException",
        "The conditional request failed",
    );
    match (str_of(body, "ReturnValuesOnConditionCheckFailure"), old) {
        (Some("ALL_OLD"), Some(item)) => e.with_body("Item", item_to_json(item).to_string()),
        _ => e,
    }
}

/// Checks the condition against the item as it is (an absent item is the empty one).
pub(crate) fn check(cond: Option<&Cond>, old: Option<&Item>, body: &Json) -> Result<(), ApiError> {
    let Some(c) = cond else { return Ok(()) };
    let empty = Item::new();
    if c.eval(old.unwrap_or(&empty)) {
        Ok(())
    } else {
        Err(failed(body, old))
    }
}

/// `ReturnValues`, held to the values `allowed` for the operation.
pub(crate) fn return_values<'a>(body: &'a Json, allowed: &[&str]) -> Result<&'a str, ApiError> {
    let given = str_of(body, "ReturnValues").unwrap_or("NONE");
    if allowed.contains(&given) {
        return Ok(given);
    }
    Err(bad_param(&format!(
        "ReturnValues can only be {}",
        if allowed.len() == 2 {
            "ALL_OLD or NONE"
        } else {
            "ALL_OLD, UPDATED_OLD, ALL_NEW, UPDATED_NEW or NONE"
        }
    )))
}

/// The item narrowed to a projection.
pub(crate) fn narrowed(item: &Item, paths: Option<&[Path]>) -> Item {
    match paths {
        Some(p) => project(item, p),
        None => item.clone(),
    }
}

/// An item from the `Item` or `Key` member of a request.
pub(crate) fn item_member(body: &Json, member: &str) -> Result<Item, ApiError> {
    let raw = body.get(member).cloned().unwrap_or_else(|| json!({}));
    item_from_json(&raw).map_err(inv)
}

/// The capacity report a request asked for. `(memory)`: a write costs a unit per kilobyte (rounded up), a consistent read one per four.
pub(crate) fn consumed(
    body: &Json,
    table: &str,
    bytes: usize,
    write: bool,
    consistent: bool,
) -> Option<Json> {
    match str_of(body, "ReturnConsumedCapacity") {
        Some("TOTAL" | "INDEXES") => {}
        _ => return None,
    }
    let kb = if write { 1024 } else { 4096 };
    #[allow(clippy::cast_precision_loss)]
    let mut units = bytes.div_ceil(kb).max(1) as f64;
    if !write && !consistent {
        units /= 2.0;
    }
    Some(json!({"TableName": table, "CapacityUnits": units}))
}
