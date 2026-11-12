//! BatchGetItem and BatchWriteItem.

use std::collections::BTreeSet;

use aws_expr::item_to_json;
use aws_wire::ApiError;
use serde_json::{Map, Value as Json, json};

use crate::condition::{item_member, narrowed, projection, used};
use crate::core::{Core, Cx};
use crate::req::{Exprs, array_of, finish, table_name, validation};
use crate::stage::{item_key, key_bytes, validate_item};

const MAX_GET: usize = 100;
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
                "Too many items requested for the BatchGetItem call",
            ));
        }
        let mut responses = Map::new();
        for (given, request) in requests {
            let t = self.table(table_name(given, cx.region(), cx.account())?)?;
            let ex = Exprs::of(request)?;
            let mut scope = ex.scope();
            let paths = projection(request, &mut scope)?;
            finish(
                &scope,
                &used(request, &["ProjectionExpression"]),
                "ProjectionExpression",
            )?;
            let mut seen = BTreeSet::new();
            let mut found = Vec::new();
            for raw in array_of(request, "Keys") {
                let key = item_member(&json!({ "Key": raw }), "Key")?;
                let pk = key_bytes(&t, &key)?;
                if !seen.insert(pk.clone()) {
                    return Err(duplicates());
                }
                if let Some(item) = self.read_item(&item_key(&t, &pk))? {
                    found.push(item_to_json(&narrowed(&item, paths.as_deref())));
                }
            }
            responses.insert(given.clone(), Json::Array(found));
        }
        Ok(json!({"Responses": responses, "UnprocessedKeys": {}}))
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
                "Too many items requested for the BatchWriteItem call",
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
