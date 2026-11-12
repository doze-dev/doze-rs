//! Reading a request: its JSON body (already checked against the model by the gateway), the expression names and values, the
//! members that name a table, and the legacy parameters this service refuses.

use aws_expr::{Invalid, NameMap, Scope, Value, ValueMap};
use aws_model_dynamodb::Operation;
use aws_wire::ApiError;
use serde_json::Value as Json;

/// A `ValidationException`.
pub(crate) fn validation(message: impl Into<String>) -> ApiError {
    ApiError::new(400, "ValidationException", message)
}

/// An expression-engine refusal as the error the caller sees.
pub(crate) fn inv(e: Invalid) -> ApiError {
    validation(e.0)
}

/// `ValidationException` for a parameter value: AWS's `One or more parameter values were invalid: ...`.
pub(crate) fn bad_param(message: &str) -> ApiError {
    validation(format!(
        "One or more parameter values were invalid: {message}"
    ))
}

/// The error for a table or index that is not there. The item operations say only this; the table administration ones name the
/// table (recorded for both).
pub(crate) fn not_found() -> ApiError {
    ApiError::modeled("ResourceNotFoundException", "Requested resource not found")
}

pub(crate) fn table_not_found(name: &str) -> ApiError {
    ApiError::modeled(
        "ResourceNotFoundException",
        format!("Requested resource not found: Table: {name} not found"),
    )
}

/// The body as JSON.
pub(crate) fn body(text: &str) -> Result<Json, ApiError> {
    serde_json::from_str(text).map_err(|_| validation("Unable to parse the request body."))
}

pub(crate) fn str_of<'a>(v: &'a Json, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Json::as_str)
}

pub(crate) fn bool_of(v: &Json, key: &str) -> Option<bool> {
    v.get(key).and_then(Json::as_bool)
}

pub(crate) fn int_of(v: &Json, key: &str) -> Option<i64> {
    v.get(key).and_then(Json::as_i64)
}

pub(crate) fn array_of<'a>(v: &'a Json, key: &str) -> &'a [Json] {
    v.get(key)
        .and_then(Json::as_array)
        .map_or(&[], Vec::as_slice)
}

/// A table given by name or by ARN, as the name. An ARN of another region or account names nothing here.
pub(crate) fn table_name<'a>(
    given: &'a str,
    region: &str,
    account: &str,
) -> Result<&'a str, ApiError> {
    let Some(rest) = given.strip_prefix("arn:") else {
        return Ok(given);
    };
    let mut parts = rest.splitn(5, ':');
    let (_partition, service, r, a, resource) = (
        parts.next(),
        parts.next(),
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    );
    match resource.strip_prefix("table/") {
        Some(name) if service == Some("dynamodb") && r == region && a == account => {
            Ok(name.split('/').next().unwrap_or(name))
        }
        _ => Err(not_found()),
    }
}

/// The table ARN.
pub(crate) fn table_arn(region: &str, account: &str, name: &str) -> String {
    format!("arn:aws:dynamodb:{region}:{account}:table/{name}")
}

/// `ExpressionAttributeNames` and `ExpressionAttributeValues` of a request.
#[derive(Debug, Default)]
pub(crate) struct Exprs {
    pub(crate) names: NameMap,
    pub(crate) values: ValueMap,
}

impl Exprs {
    /// Reads the two maps of `v`, refusing keys that are not placeholders and values that are not attribute values.
    pub(crate) fn of(v: &Json) -> Result<Self, ApiError> {
        let mut out = Self::default();
        if let Some(Json::Object(m)) = v.get("ExpressionAttributeNames") {
            if m.is_empty() {
                return Err(validation("ExpressionAttributeNames must not be empty"));
            }
            for (k, name) in m {
                let Some(name) = name.as_str() else { continue };
                if !k.starts_with('#') || k.len() < 2 {
                    return Err(validation(format!(
                        "ExpressionAttributeNames contains invalid key: Syntax error; key: \"{k}\""
                    )));
                }
                out.names.insert(k.clone(), name.to_owned());
            }
        }
        if let Some(Json::Object(m)) = v.get("ExpressionAttributeValues") {
            if m.is_empty() {
                return Err(validation("ExpressionAttributeValues must not be empty"));
            }
            for (k, raw) in m {
                if !k.starts_with(':') || k.len() < 2 {
                    return Err(validation(format!(
                        "ExpressionAttributeValues contains invalid key: Syntax error; key: \"{k}\""
                    )));
                }
                out.values
                    .insert(k.clone(), Value::from_json(raw).map_err(inv)?);
            }
        }
        Ok(out)
    }

    pub(crate) fn scope(&self) -> Scope<'_> {
        Scope::new(&self.names, &self.values)
    }
}

/// Ends a request's expressions: with none given, names and values are an error; with some, every one must be used.
pub(crate) fn finish(scope: &Scope<'_>, used: &[&str], label: &str) -> Result<(), ApiError> {
    if used.is_empty() {
        if scope.has_names() {
            return Err(validation(format!(
                "ExpressionAttributeNames can only be specified when using expressions: {label} is null"
            )));
        }
        if scope.has_values() {
            return Err(validation(format!(
                "ExpressionAttributeValues can only be specified when using expressions: {label} is null"
            )));
        }
        return Ok(());
    }
    scope.finish().map_err(inv)
}

/// What each legacy parameter is replaced by.
const LEGACY: &[(&str, &str)] = &[
    ("Expected", "ConditionExpression"),
    ("ConditionalOperator", "ConditionExpression"),
    ("AttributeUpdates", "UpdateExpression"),
    ("AttributesToGet", "ProjectionExpression"),
    ("KeyConditions", "KeyConditionExpression"),
    ("QueryFilter", "FilterExpression"),
    ("ScanFilter", "FilterExpression"),
];

/// Refuses the legacy parameters by name: they are built by no one's hand here, and ignoring them would make a conditional write
/// unconditional. `(memory)` for the wording, which is doze's own.
pub(crate) fn refuse_legacy(op: Operation, body: &Json) -> Result<(), ApiError> {
    let refuse = |name: &str, instead: &str| {
        ApiError::new(
            501,
            "NotImplemented",
            format!(
                "The legacy parameter {name} of {op:?} is not built in doze-aws; use {instead}."
            ),
        )
    };
    let check = |v: &Json| -> Result<(), ApiError> {
        for (name, instead) in LEGACY {
            if v.get(*name).is_some() {
                return Err(refuse(name, instead));
            }
        }
        Ok(())
    };
    if matches!(op, Operation::BatchGetItem) {
        if let Some(Json::Object(tables)) = body.get("RequestItems") {
            for t in tables.values() {
                check(t)?;
            }
        }
        return Ok(());
    }
    check(body)
}

/// Sets a member of a JSON object.
pub(crate) fn set(target: &mut Json, key: &str, value: Json) {
    if let Json::Object(m) = target {
        m.insert(key.to_owned(), value);
    }
}
