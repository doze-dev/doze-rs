//! The comparisons of the legacy parameters (`QueryFilter`, `ScanFilter`, `Expected`, `KeyConditions` entries): operator
//! arity and types checked, then written as expression text.

use aws_wire::ApiError;
use serde_json::Value as Json;

use super::{Builder, detected_param};
use crate::req::bad_param;

/// One entry of a `QueryFilter` or `ScanFilter`: `{"ComparisonOperator", "AttributeValueList"}`.
pub(super) fn filter(b: &mut Builder, attr: &str, cond: &Json) -> Result<String, ApiError> {
    let op = cond
        .get("ComparisonOperator")
        .and_then(Json::as_str)
        .unwrap_or_default();
    comparison(b, attr, op, values_of(cond))
}

pub(super) fn values_of(cond: &Json) -> &[Json] {
    cond.get("AttributeValueList")
        .and_then(Json::as_array)
        .map_or(&[], Vec::as_slice)
}

/// One entry of `Expected`: a comparison, or `Value` and `Exists`.
pub(super) fn expected(b: &mut Builder, attr: &str, cond: &Json) -> Result<String, ApiError> {
    let value = cond.get("Value");
    let exists = cond.get("Exists").and_then(Json::as_bool);
    if let Some(op) = cond.get("ComparisonOperator").and_then(Json::as_str) {
        if value.is_some() || exists.is_some() {
            return Err(bad_param(&format!(
                "Value or Exists cannot be used with ComparisonOperator for Attribute: {attr}"
            )));
        }
        return comparison(b, attr, op, values_of(cond));
    }
    if cond.get("AttributeValueList").is_some() {
        return Err(bad_param(&format!(
            "AttributeValueList can only be used with a ComparisonOperator for Attribute: {attr}"
        )));
    }
    match (value, exists.unwrap_or(true)) {
        (Some(v), true) => {
            let (n, v) = (b.name(attr), b.value(v));
            Ok(format!("{n} = {v}"))
        }
        // Recorded 2026-11-19 (both).
        (Some(_), false) => Err(detected_param(&format!(
            "Value cannot be used when Exists is false for Attribute: {attr}"
        ))),
        (None, true) => Err(detected_param(&format!(
            "Value must be provided when Exists is true for Attribute: {attr}"
        ))),
        (None, false) => Ok(format!("attribute_not_exists({})", b.name(attr))),
    }
}

/// How many values each operator takes: exactly `n`, or (for `IN`) at least one.
fn arity(op: &str) -> Option<(usize, bool)> {
    Some(match op {
        "NULL" | "NOT_NULL" => (0, false),
        "EQ" | "NE" | "LE" | "LT" | "GE" | "GT" | "CONTAINS" | "NOT_CONTAINS" | "BEGINS_WITH" => {
            (1, false)
        }
        "BETWEEN" => (2, false),
        "IN" => (1, true),
        _ => return None,
    })
}

/// The type of an attribute value as given: `{"S": ...}` is `S`.
fn type_of(v: &Json) -> &str {
    v.as_object()
        .and_then(|m| m.keys().next())
        .map_or("", String::as_str)
}

/// The types an operator compares (the guide: ordering and `BEGINS_WITH` on scalars only).
fn check_types(op: &str, values: &[Json]) -> Result<(), ApiError> {
    let allowed: &[&str] = match op {
        "LE" | "LT" | "GE" | "GT" | "BETWEEN" | "CONTAINS" | "NOT_CONTAINS" | "IN" => {
            &["S", "N", "B"]
        }
        "BEGINS_WITH" => &["S", "B"],
        _ => return Ok(()),
    };
    for v in values {
        let ty = type_of(v);
        if !allowed.contains(&ty) {
            return Err(bad_param(&format!(
                "ComparisonOperator {op} is not valid for {ty} AttributeValue type"
            )));
        }
    }
    if op == "BETWEEN"
        && let [lo, hi] = values
        && type_of(lo) != type_of(hi)
    {
        return Err(bad_param(
            "AttributeValues inside AttributeValueList must be of same type",
        ));
    }
    Ok(())
}

/// A comparison as an expression.
pub(super) fn comparison(
    b: &mut Builder,
    attr: &str,
    op: &str,
    values: &[Json],
) -> Result<String, ApiError> {
    let (n, at_least) =
        arity(op).ok_or_else(|| bad_param(&format!("Unsupported ComparisonOperator: {op}")))?;
    let count_ok = if at_least {
        values.len() >= n
    } else {
        values.len() == n
    };
    if !count_ok {
        return Err(bad_param(&format!(
            "Invalid number of argument(s) for the {op} ComparisonOperator"
        )));
    }
    check_types(op, values)?;
    let name = b.name(attr);
    let vals: Vec<String> = values.iter().map(|v| b.value(v)).collect();
    let first = vals.first().cloned().unwrap_or_default();
    Ok(match op {
        "EQ" => format!("{name} = {first}"),
        "NE" => format!("{name} <> {first}"),
        "LE" => format!("{name} <= {first}"),
        "LT" => format!("{name} < {first}"),
        "GE" => format!("{name} >= {first}"),
        "GT" => format!("{name} > {first}"),
        "NULL" => format!("attribute_not_exists({name})"),
        "NOT_NULL" => format!("attribute_exists({name})"),
        "CONTAINS" => format!("contains({name}, {first})"),
        "NOT_CONTAINS" => format!("NOT contains({name}, {first})"),
        "BEGINS_WITH" => format!("begins_with({name}, {first})"),
        "IN" => format!("{name} IN ({})", vals.join(", ")),
        _ => format!(
            "{name} BETWEEN {first} AND {}",
            vals.get(1).cloned().unwrap_or_default()
        ),
    })
}
