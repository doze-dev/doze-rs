//! The legacy request parameters older SDKs and libraries still send (`AttributesToGet`, `KeyConditions`, `QueryFilter`,
//! `ScanFilter`, `Expected`, `ConditionalOperator`, `AttributeUpdates`), translated before the call is handled into the expressions
//! they stand for, with placeholders of their own (`#l0`, `:l0`) so no attribute name can clash with a reserved word. What runs is
//! then the same code an expression request runs.
//!
//! The rules are the developer guide's ("Legacy conditional parameters"), as AWS applies them (recorded 2026-11-19): a legacy
//! parameter is refused beside the expression that does its job (`Expected` with `ConditionExpression`), while other mixes
//! (`KeyConditions` with a `FilterExpression`) are accepted, though the guide says mixing is not allowed; each
//! comparison operator takes the number of values the guide lists, `Exists: false` is "the attribute is absent", a bare `Value` (or
//! `Exists: true` with one) is equality, and an `AttributeUpdates` of only `DELETE` actions on an item that does not exist changes
//! nothing ("The operation succeeds, but DynamoDB does not create a new item"). A message marked `Recorded 2026-11-19` is AWS's;
//! the rest are `(memory)` until a recording reaches them.

use aws_model_dynamodb::Operation;
use aws_wire::ApiError;
use serde_json::{Map, Value as Json, json};

use crate::req::{bad_param, validation};

/// AWS's wording for some legacy refusals: the message behind `1 validation error detected: ` (recorded 2026-11-19 for
/// `PutItem`, `UpdateItem` and `Scan`).
fn detected(message: &str) -> ApiError {
    validation(format!("1 validation error detected: {message}"))
}

/// [`detected`] around a [`bad_param`] message.
fn detected_param(message: &str) -> ApiError {
    detected(&format!(
        "One or more parameter values were invalid: {message}"
    ))
}

/// Set on a translated `UpdateItem` whose actions are all `DELETE`: an item that is not there is not made.
pub(crate) const NO_CREATE: &str = "__dozeLegacyDeleteOnly";

/// The expression parameters, in the order a refusal names them.
const EXPRESSIONS: &[&str] = &[
    "ProjectionExpression",
    "KeyConditionExpression",
    "FilterExpression",
    "ConditionExpression",
    "UpdateExpression",
];

/// The legacy parameters each operation takes.
fn legacy_of(op: Operation) -> &'static [&'static str] {
    match op {
        Operation::GetItem => &["AttributesToGet"],
        Operation::PutItem | Operation::DeleteItem => &["Expected", "ConditionalOperator"],
        Operation::UpdateItem => &["AttributeUpdates", "Expected", "ConditionalOperator"],
        Operation::Query => &[
            "AttributesToGet",
            "KeyConditions",
            "QueryFilter",
            "ConditionalOperator",
        ],
        Operation::Scan => &["AttributesToGet", "ScanFilter", "ConditionalOperator"],
        _ => &[],
    }
}

/// Rewrites the legacy parameters of a request as expressions.
pub(crate) fn translate(op: Operation, mut body: Json) -> Result<Json, ApiError> {
    if op == Operation::BatchGetItem {
        if let Some(Json::Object(tables)) = body.get_mut("RequestItems") {
            for entry in tables.values_mut() {
                if let Json::Object(m) = entry {
                    one(&["AttributesToGet"], op, m)?;
                }
            }
        }
        return Ok(body);
    }
    if let Json::Object(m) = &mut body {
        one(legacy_of(op), op, m)?;
    }
    Ok(body)
}

/// Placeholders for the names and values a translation uses.
#[derive(Default)]
struct Builder {
    names: Map<String, Json>,
    values: Map<String, Json>,
}

impl Builder {
    fn name(&mut self, attr: &str) -> String {
        let key = format!("#l{}", self.names.len());
        self.names.insert(key.clone(), json!(attr));
        key
    }

    fn value(&mut self, v: &Json) -> String {
        let key = format!(":l{}", self.values.len());
        self.values.insert(key.clone(), v.clone());
        key
    }
}

fn one(legacy: &[&str], op: Operation, m: &mut Map<String, Json>) -> Result<(), ApiError> {
    let given: Vec<&str> = legacy
        .iter()
        .copied()
        .filter(|p| m.contains_key(*p))
        .collect();
    if given.is_empty() {
        return Ok(());
    }
    let expressions: Vec<&str> = EXPRESSIONS
        .iter()
        .copied()
        .filter(|p| m.contains_key(*p))
        .collect();
    // Recorded 2026-11-19: a legacy parameter is refused beside the expression that does its job (`AttributesToGet` with
    // `ProjectionExpression`, `Expected` with `ConditionExpression`), but `KeyConditions` with a `FilterExpression` is accepted.
    let clash = given
        .iter()
        .any(|l| counterparts(op, l).iter().any(|e| m.contains_key(*e)));
    if clash {
        return Err(mixed(op, &given, &expressions));
    }
    if expressions.is_empty() {
        for placeholders in ["ExpressionAttributeNames", "ExpressionAttributeValues"] {
            if m.contains_key(placeholders) {
                return Err(validation(format!(
                    "{placeholders} can only be specified when using expressions"
                )));
            }
        }
    }
    let mut b = Builder::default();
    if let Some(attrs) = m.remove("AttributesToGet") {
        let text = projection(&mut b, m, &attrs)?;
        m.insert("ProjectionExpression".into(), json!(text));
    }
    if let Some(keys) = m.remove("KeyConditions") {
        let text = key_conditions(&mut b, &keys)?;
        m.insert("KeyConditionExpression".into(), json!(text));
    }
    conditions(&mut b, op, m)?;
    if let Some(updates) = m.remove("AttributeUpdates") {
        let (text, delete_only) = attribute_updates(&mut b, &updates)?;
        if let Some(t) = text {
            m.insert("UpdateExpression".into(), json!(t));
        }
        if delete_only {
            m.insert(NO_CREATE.into(), json!(true));
        }
    }
    // Beside an expression the caller's own placeholders stay; the translation's (`#l0`, `:l0`) join them.
    for (key, made) in [
        ("ExpressionAttributeNames", b.names),
        ("ExpressionAttributeValues", b.values),
    ] {
        if made.is_empty() {
            continue;
        }
        match m.get_mut(key) {
            Some(Json::Object(own)) => own.extend(made),
            _ => {
                m.insert(key.into(), Json::Object(made));
            }
        }
    }
    Ok(())
}

/// The refusal of legacy and expression parameters together. Recorded 2026-11-19: the write operations say it behind
/// `1 validation error detected: `, `GetItem` without (`DeleteItem` is taken to answer as `PutItem` does).
fn mixed(op: Operation, given: &[&str], expressions: &[&str]) -> ApiError {
    let message = format!(
        "Can not use both expression and non-expression parameters in the same request: Non-expression parameters: {{{}}} Expression parameters: {{{}}}",
        given.join(", "),
        expressions.join(", ")
    );
    if matches!(
        op,
        Operation::PutItem | Operation::UpdateItem | Operation::DeleteItem
    ) {
        detected(&message)
    } else {
        validation(message)
    }
}

/// The expression parameters that do a legacy parameter's job: the two may not be sent together.
fn counterparts(op: Operation, legacy: &str) -> &'static [&'static str] {
    match legacy {
        "AttributesToGet" => &["ProjectionExpression"],
        "KeyConditions" => &["KeyConditionExpression"],
        "QueryFilter" | "ScanFilter" => &["FilterExpression"],
        "Expected" => &["ConditionExpression"],
        "AttributeUpdates" => &["UpdateExpression"],
        "ConditionalOperator" if matches!(op, Operation::Query | Operation::Scan) => {
            &["FilterExpression"]
        }
        "ConditionalOperator" => &["ConditionExpression"],
        _ => &[],
    }
}

/// `AttributesToGet` as a `ProjectionExpression`.
fn projection(b: &mut Builder, m: &Map<String, Json>, attrs: &Json) -> Result<String, ApiError> {
    if let Some(select) = m.get("Select").and_then(Json::as_str)
        && select != "SPECIFIC_ATTRIBUTES"
    {
        // Recorded 2026-11-19.
        return Err(detected(&format!(
            "Cannot specify the AttributesToGet when choosing to get {select}"
        )));
    }
    let list: Vec<&str> = attrs
        .as_array()
        .map(|a| a.iter().filter_map(Json::as_str).collect())
        .unwrap_or_default();
    let mut seen = std::collections::BTreeSet::new();
    let mut parts = Vec::new();
    for a in list {
        if !seen.insert(a) {
            return Err(bad_param(&format!(
                "Duplicate value in attribute name: {a}"
            )));
        }
        parts.push(b.name(a));
    }
    Ok(parts.join(", "))
}

/// `QueryFilter`, `ScanFilter` and `Expected`, with `ConditionalOperator`, as a filter or condition expression.
fn conditions(b: &mut Builder, op: Operation, m: &mut Map<String, Json>) -> Result<(), ApiError> {
    let joiner = m.remove("ConditionalOperator");
    let (source, target) = match op {
        Operation::Query => ("QueryFilter", "FilterExpression"),
        Operation::Scan => ("ScanFilter", "FilterExpression"),
        _ => ("Expected", "ConditionExpression"),
    };
    let Some(Json::Object(entries)) = m.remove(source) else {
        if joiner.is_some() {
            return Err(only_with_two());
        }
        return Ok(());
    };
    let join = match joiner.as_ref().and_then(Json::as_str) {
        Some("OR") => " OR ",
        _ => " AND ",
    };
    if joiner.is_some() && entries.len() < 2 {
        return Err(only_with_two());
    }
    let mut parts = Vec::new();
    for (attr, cond) in &entries {
        parts.push(if source == "Expected" {
            expected(b, attr, cond)?
        } else {
            filter(b, attr, cond)?
        });
    }
    if parts.is_empty() {
        return Ok(());
    }
    let text = if parts.len() == 1 {
        parts.concat()
    } else {
        parts
            .iter()
            .map(|p| format!("({p})"))
            .collect::<Vec<_>>()
            .join(join)
    };
    m.insert(target.into(), json!(text));
    Ok(())
}

fn only_with_two() -> ApiError {
    // Recorded 2026-11-19.
    validation(
        "ConditionalOperator can only be used when Filter or Expected has two or more elements",
    )
}

/// One entry of a `QueryFilter` or `ScanFilter`: `{"ComparisonOperator", "AttributeValueList"}`.
fn filter(b: &mut Builder, attr: &str, cond: &Json) -> Result<String, ApiError> {
    let op = cond
        .get("ComparisonOperator")
        .and_then(Json::as_str)
        .unwrap_or_default();
    comparison(b, attr, op, values_of(cond))
}

fn values_of(cond: &Json) -> &[Json] {
    cond.get("AttributeValueList")
        .and_then(Json::as_array)
        .map_or(&[], Vec::as_slice)
}

/// One entry of `Expected`: a comparison, or `Value` and `Exists`.
fn expected(b: &mut Builder, attr: &str, cond: &Json) -> Result<String, ApiError> {
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
fn comparison(b: &mut Builder, attr: &str, op: &str, values: &[Json]) -> Result<String, ApiError> {
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

/// `KeyConditions` as a `KeyConditionExpression`: the operators a key can be read by only.
fn key_conditions(b: &mut Builder, keys: &Json) -> Result<String, ApiError> {
    let Some(entries) = keys.as_object() else {
        return Ok(String::new());
    };
    let mut parts = Vec::new();
    for (attr, cond) in entries {
        let op = cond
            .get("ComparisonOperator")
            .and_then(Json::as_str)
            .unwrap_or_default();
        if !matches!(
            op,
            "EQ" | "LE" | "LT" | "GE" | "GT" | "BEGINS_WITH" | "BETWEEN"
        ) {
            // Recorded 2026-11-19.
            return Err(bad_param(
                "Attempted conditional constraint is not an indexable operation",
            ));
        }
        parts.push(comparison(b, attr, op, values_of(cond))?);
    }
    Ok(parts.join(" AND "))
}

/// `AttributeUpdates` as an `UpdateExpression`, and whether every action is a `DELETE`.
fn attribute_updates(b: &mut Builder, updates: &Json) -> Result<(Option<String>, bool), ApiError> {
    let Some(entries) = updates.as_object() else {
        return Ok((None, false));
    };
    let (mut set, mut remove, mut add, mut delete) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (attr, u) in entries {
        let action = u.get("Action").and_then(Json::as_str).unwrap_or("PUT");
        let value = u.get("Value");
        match (action, value) {
            ("DELETE", None) => remove.push(b.name(attr)),
            ("DELETE", Some(v)) => {
                let (n, v) = (b.name(attr), b.value(v));
                delete.push(format!("{n} {v}"));
            }
            (_, None) => {
                // Recorded 2026-11-19.
                return Err(detected_param(
                    "Only DELETE action is allowed when no attribute value is specified",
                ));
            }
            ("ADD", Some(v)) => {
                let (n, v) = (b.name(attr), b.value(v));
                add.push(format!("{n} {v}"));
            }
            (_, Some(v)) => {
                let (n, v) = (b.name(attr), b.value(v));
                set.push(format!("{n} = {v}"));
            }
        }
    }
    let delete_only = set.is_empty() && add.is_empty() && !(remove.is_empty() && delete.is_empty());
    let mut clauses = Vec::new();
    for (word, list) in [
        ("SET", set),
        ("REMOVE", remove),
        ("ADD", add),
        ("DELETE", delete),
    ] {
        if !list.is_empty() {
            clauses.push(format!("{word} {}", list.join(", ")));
        }
    }
    Ok((
        (!clauses.is_empty()).then(|| clauses.join(" ")),
        delete_only,
    ))
}
