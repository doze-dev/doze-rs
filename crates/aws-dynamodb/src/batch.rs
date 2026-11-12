//! BatchGetItem and BatchWriteItem.
//!
//! Wording: the messages in `conformance/snapshots/test_dynamodb.json` (recorded 2026-11-09) are AWS's own; every other message in this file is
//! `(memory)`: from the developer guide and from use, and unchecked until a recording reaches it.

use std::collections::BTreeSet;

use aws_expr::{item_size, item_to_json};
use aws_wire::ApiError;
use serde_json::{Map, Value as Json, json};

use crate::condition::{item_member, narrowed, projection, used};
use crate::core::{Core, Cx};
use crate::req::{Exprs, array_of, finish, set, table_name, validation};
use crate::stage::{item_key, key_bytes, validate_item};

const MAX_GET: usize = 100;
/// A response stops taking items at 16 MB (the model's documentation).
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_WRITE: usize = 25;

fn duplicates() -> ApiError {
    validation("Provided list of item keys contains duplicates")
}

impl Core {
    pub(crate) fn batch_get(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let Some(Json::Object(requests)) = cx.body.get("RequestItems") else {
            return Err(validation("RequestItems is required"));
        };
        let total: usize = requests.values().map(|r| array_of(r, "Keys").len()).sum();
        if total > MAX_GET {
            return Err(validation(
                "Too many items requested for the BatchGetItem call.",
            ));
        }
        let (mut responses, mut unprocessed, mut bytes) = (Map::new(), Map::new(), 0);
        for (given, request) in requests {
            let (found, left) = self.get_table(cx, given, request, &mut bytes)?;
            responses.insert(given.clone(), Json::Array(found));
            if !left.is_empty() {
                let mut entry = request.clone();
                set(&mut entry, "Keys", Json::Array(left));
                unprocessed.insert(given.clone(), entry);
            }
        }
        Ok(json!({"Responses": responses, "UnprocessedKeys": unprocessed}))
    }

    /// The items of one table's keys, and the keys left for the caller to ask again once the response holds 16 MB.
    fn get_table(
        &self,
        cx: &Cx<'_>,
        given: &str,
        request: &Json,
        bytes: &mut usize,
    ) -> Result<(Vec<Json>, Vec<Json>), ApiError> {
        let t = self.table(table_name(given, cx.region(), cx.account())?)?;
        let ex = Exprs::of(request)?;
        let mut scope = ex.scope();
        let paths = projection(request, &mut scope)?;
        finish(
            &scope,
            &used(request, &["ProjectionExpression"]),
            "ProjectionExpression",
        )?;
        let (mut seen, mut found, mut left) = (BTreeSet::new(), Vec::new(), Vec::new());
        for raw in array_of(request, "Keys") {
            let pk = key_bytes(&t, &item_member(&json!({ "Key": raw }), "Key")?)?;
            if !seen.insert(pk.clone()) {
                return Err(duplicates());
            }
            if *bytes >= MAX_RESPONSE_BYTES {
                left.push(raw.clone());
            } else if let Some(item) = self.read_item(&item_key(&t, &pk))? {
                *bytes += item_size(&item);
                found.push(item_to_json(&narrowed(&item, paths.as_deref())));
            }
        }
        Ok((found, left))
    }

    pub(crate) fn batch_write(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let Some(Json::Object(requests)) = cx.body.get("RequestItems") else {
            return Err(validation("RequestItems is required"));
        };
        let total: usize = requests
            .values()
            .map(|r| r.as_array().map_or(0, Vec::len))
            .sum();
        if total > MAX_WRITE {
            return Err(validation(
                "Too many items requested for the BatchWriteItem call.",
            ));
        }
        // Everything is checked before anything is staged: a batch with one bad request writes nothing.
        let mut planned = Vec::new();
        for (given, writes) in requests {
            let t = self.table(table_name(given, cx.region(), cx.account())?)?;
            let mut seen = BTreeSet::new();
            for w in writes.as_array().map_or(&[][..], Vec::as_slice) {
                let (item, pk) = match (w.get("PutRequest"), w.get("DeleteRequest")) {
                    (Some(p), None) => {
                        let item = item_member(p, "Item")?;
                        let pk = validate_item(&t, &item)?;
                        (Some(item), pk)
                    }
                    (None, Some(d)) => (None, key_bytes(&t, &item_member(d, "Key")?)?),
                    _ => {
                        return Err(validation(
                            "Supplied AttributeValue has more than one datatypes set, must contain exactly one of the supported datatypes",
                        ));
                    }
                };
                if !seen.insert(pk.clone()) {
                    return Err(duplicates());
                }
                planned.push((t.clone(), pk, item));
            }
        }
        for (t, pk, item) in planned {
            let old = self.read_item(&item_key(&t, &pk))?;
            if item.is_some() || old.is_some() {
                self.stage(&t, cx.now_ms, old.as_ref(), item.as_ref(), false)?;
            }
        }
        Ok(json!({"UnprocessedItems": {}}))
    }
}
