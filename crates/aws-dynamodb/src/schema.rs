//! Reading a table's schema from a request: attribute definitions, key schemas, projections, indexes and billing. The refusals'
//! wording is `(memory)` unless said otherwise: no recording reaches a malformed schema.

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::Value as Json;

use crate::meta::{Billing, Index, Key, Projection, Status};
use crate::req::{array_of, bad_param, bool_of, int_of, str_of, validation};

/// The most local and global secondary indexes a table has, and the most non-key attributes projected across them all.
pub(crate) const MAX_LSI: usize = 5;
pub(crate) const MAX_GSI: usize = 20;
pub(crate) const MAX_PROJECTED: usize = 100;

/// `AttributeDefinitions` as a map from name to type (`S`, `N`, `B`).
pub(crate) fn definitions(v: &Json) -> Result<BTreeMap<String, char>, ApiError> {
    let mut out = BTreeMap::new();
    for d in array_of(v, "AttributeDefinitions") {
        let name = str_of(d, "AttributeName").unwrap_or_default();
        let ty = str_of(d, "AttributeType")
            .and_then(|t| t.chars().next())
            .unwrap_or('S');
        if out.insert(name.to_owned(), ty).is_some() {
            return Err(bad_param(&format!("Duplicate Attribute Name: {name}")));
        }
    }
    Ok(out)
}

/// A key schema: the partition key, then the optional sort key, each with its type from `defs`.
pub(crate) fn key_schema(
    elems: &[Json],
    defs: &BTreeMap<String, char>,
    allow_two: bool,
) -> Result<(Key, Option<Key>), ApiError> {
    let kind = |e: &Json| str_of(e, "KeyType").unwrap_or_default().to_owned();
    let name = |e: &Json| str_of(e, "AttributeName").unwrap_or_default().to_owned();
    let key = |e: &Json| -> Result<Key, ApiError> {
        let n = name(e);
        let ty = *defs.get(&n).ok_or_else(|| {
            let keys: Vec<String> = elems.iter().map(name).collect();
            let known: Vec<&String> = defs.keys().collect();
            bad_param(&format!(
                "Some index key attributes are not defined in AttributeDefinitions. Keys: [{}], AttributeDefinitions: [{}]",
                keys.join(", "),
                known.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            ))
        })?;
        Ok(Key { name: n, ty })
    };
    match elems {
        [] => Err(validation(
            "Invalid KeySchema: The KeySchema must have at least one element",
        )),
        [h] | [h, _] if kind(h) != "HASH" => Err(validation(
            "Invalid KeySchema: The first KeySchemaElement is not a HASH key type",
        )),
        [h] => Ok((key(h)?, None)),
        [h, r] if allow_two => {
            if kind(r) != "RANGE" {
                return Err(validation(
                    "Invalid KeySchema: The second KeySchemaElement is not a RANGE key type",
                ));
            }
            if name(h) == name(r) {
                return Err(validation(
                    "Invalid KeySchema: Duplicate key attribute name",
                ));
            }
            Ok((key(h)?, Some(key(r)?)))
        }
        _ => Err(validation(
            "Invalid KeySchema: The KeySchema may have at most 2 elements",
        )),
    }
}

/// The projection of an index.
pub(crate) fn projection(v: Option<&Json>, index: &str) -> Result<Projection, ApiError> {
    let Some(p) = v else {
        return Err(validation(format!(
            "One or more parameter values were invalid: Projection is required for index {index}"
        )));
    };
    let non_key: Vec<String> = array_of(p, "NonKeyAttributes")
        .iter()
        .filter_map(Json::as_str)
        .map(str::to_owned)
        .collect();
    match str_of(p, "ProjectionType") {
        Some("ALL") => Ok(Projection::All),
        Some("KEYS_ONLY") => Ok(Projection::KeysOnly),
        Some("INCLUDE") if non_key.is_empty() => Err(bad_param(
            "One or more NonKeyAttributes are required when ProjectionType is INCLUDE",
        )),
        Some("INCLUDE") => Ok(Projection::Include(non_key)),
        _ => Err(bad_param("Projection type is required")),
    }
}

/// `ProvisionedThroughput` as read and write units.
pub(crate) fn throughput(v: Option<&Json>) -> Option<(i64, i64)> {
    let p = v?;
    Some((
        int_of(p, "ReadCapacityUnits")?,
        int_of(p, "WriteCapacityUnits")?,
    ))
}

/// Billing from `BillingMode` and `ProvisionedThroughput`.
pub(crate) fn billing(v: &Json, now_ms: i64) -> Result<Billing, ApiError> {
    let given = throughput(v.get("ProvisionedThroughput"));
    let mode = str_of(v, "BillingMode");
    match (mode, given) {
        (Some("PAY_PER_REQUEST"), Some(_)) => Err(bad_param(
            "Neither ReadCapacityUnits nor WriteCapacityUnits can be specified when BillingMode is PAY_PER_REQUEST",
        )),
        (Some("PAY_PER_REQUEST"), None) => Ok(Billing {
            pay_per_request: true,
            read_units: 0,
            write_units: 0,
            pay_per_request_since_ms: now_ms,
        }),
        (_, Some((read_units, write_units))) => Ok(Billing {
            pay_per_request: false,
            read_units,
            write_units,
            pay_per_request_since_ms: 0,
        }),
        (_, None) => Err(validation(
            "No provisioned throughput specified for the table",
        )),
    }
}

/// One index from its request form. `lsi` indexes share the table's partition key and need its sort key.
pub(crate) fn index(
    e: &Json,
    id: u32,
    lsi: Option<&Key>,
    defs: &BTreeMap<String, char>,
    pay_per_request: bool,
    now_ms: i64,
) -> Result<Index, ApiError> {
    let name = str_of(e, "IndexName").unwrap_or_default().to_owned();
    check_index_name(&name)?;
    let elems = array_of(e, "KeySchema");
    if !lsi.is_some() && elems.len() > 2 {
        return Err(ApiError::new(
            501,
            "NotImplemented",
            format!(
                "Multi-attribute keys of global secondary index {name} are not built in doze-aws."
            ),
        ));
    }
    let (hash, range) = key_schema(elems, defs, true)?;
    if let Some(table_hash) = lsi {
        if hash.name != table_hash.name {
            return Err(bad_param(&format!(
                "Index KeySchema does not have the same partition key as the table: {}",
                table_hash.name
            )));
        }
        if range.is_none() {
            return Err(bad_param(&format!(
                "Index KeySchema does not have a range key for index: {name}"
            )));
        }
    }
    let provisioned = if lsi.is_some() {
        None
    } else {
        index_throughput(e, pay_per_request, &name)?
    };
    Ok(Index {
        id,
        name: name.clone(),
        hash,
        range,
        projection: projection(e.get("Projection"), &name)?,
        provisioned,
        status: Status::Creating,
        status_until_ms: now_ms,
    })
}

fn index_throughput(
    e: &Json,
    pay_per_request: bool,
    name: &str,
) -> Result<Option<(i64, i64)>, ApiError> {
    let given = throughput(e.get("ProvisionedThroughput"));
    match (pay_per_request, given) {
        (true, Some(_)) => Err(bad_param(&format!(
            "ProvisionedThroughput should not be specified for index: {name} when BillingMode is PAY_PER_REQUEST"
        ))),
        (false, None) => Err(bad_param(&format!(
            "ProvisionedThroughput must be specified for index: {name}"
        ))),
        (_, given) => Ok(given),
    }
}

fn check_index_name(name: &str) -> Result<(), ApiError> {
    let ok_chars = name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'));
    if name.len() < 3 || name.len() > 255 || !ok_chars {
        return Err(validation(format!(
            "1 validation error detected: Value '{name}' at 'globalSecondaryIndexes.1.member.indexName' failed to satisfy constraint: Member must satisfy regular expression pattern: [a-zA-Z0-9_.-]+"
        )));
    }
    Ok(())
}

/// The table name rules: 3 to 255 of letters, digits, `_`, `.`, `-` (the model gives the request member only a length). AWS words a
/// breach like every other constraint, and reports both when both are broken.
pub(crate) fn check_table_name(name: &str) -> Result<(), ApiError> {
    let chars = name.chars().count();
    let mut problems = Vec::new();
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
        || name.is_empty()
    {
        problems.push("Member must satisfy regular expression pattern: [a-zA-Z0-9_.-]+".to_owned());
    }
    if chars < 3 {
        problems.push("Member must have length greater than or equal to 3".to_owned());
    }
    if chars > 255 {
        problems.push("Member must have length less than or equal to 255".to_owned());
    }
    if problems.is_empty() {
        return Ok(());
    }
    let n = problems.len();
    let each: Vec<String> = problems
        .into_iter()
        .map(|p| format!("Value '{name}' at 'tableName' failed to satisfy constraint: {p}"))
        .collect();
    Err(validation(format!(
        "{n} validation error{} detected: {}",
        if n == 1 { "" } else { "s" },
        each.join("; ")
    )))
}

/// `DeletionProtectionEnabled`, if given.
pub(crate) fn protection(v: &Json) -> Option<bool> {
    bool_of(v, "DeletionProtectionEnabled")
}
