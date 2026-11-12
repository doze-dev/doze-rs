//! PutItem, GetItem, UpdateItem and DeleteItem.

use aws_expr::{Item, Update, item_size, item_to_json, parse_update};
use aws_wire::ApiError;
use serde_json::{Map, Value as Json};

use crate::condition::{
    check, condition, consumed, item_member, narrowed, projection, return_values, used,
};
use crate::core::{Core, Cx};
use crate::meta::Table;
use crate::req::{Exprs, bool_of, finish, inv, str_of};
use crate::stage::{item_key, key_bytes, key_of, validate_item};

/// `{"Attributes": item}` or nothing.
fn attributes(out: &mut Map<String, Json>, item: Option<&Item>) {
    if let Some(i) = item {
        out.insert("Attributes".into(), item_to_json(i));
    }
}

fn capacity(out: &mut Map<String, Json>, cx: &Cx<'_>, t: &Table, bytes: usize, write: bool) {
    if let Some(c) = consumed(&cx.body, &t.name, bytes, write, true) {
        out.insert("ConsumedCapacity".into(), c);
    }
}

impl Core {
    pub(crate) fn put_item(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let t = self.table(&cx.table_name("TableName")?)?;
        let item = item_member(&cx.body, "Item")?;
        let returning = return_values(&cx.body, &["NONE", "ALL_OLD"])?;
        let ex = Exprs::of(&cx.body)?;
        let mut scope = ex.scope();
        let cond = condition(&cx.body, &mut scope)?;
        finish(
            &scope,
            &used(&cx.body, &["ConditionExpression"]),
            "ConditionExpression",
        )?;
        let pk = validate_item(&t, &item)?;
        let old = self.read_item(&item_key(&t, &pk))?;
        check(cond.as_ref(), old.as_ref(), &cx.body)?;
        self.stage(&t, cx.now_ms, old.as_ref(), Some(&item), false)?;
        let mut out = Map::new();
        if returning == "ALL_OLD" {
            attributes(&mut out, old.as_ref());
        }
        capacity(&mut out, cx, &t, item_size(&item), true);
        Ok(Json::Object(out))
    }

    pub(crate) fn get_item(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let t = self.table(&cx.table_name("TableName")?)?;
        let key = item_member(&cx.body, "Key")?;
        let ex = Exprs::of(&cx.body)?;
        let mut scope = ex.scope();
        let paths = projection(&cx.body, &mut scope)?;
        finish(
            &scope,
            &used(&cx.body, &["ProjectionExpression"]),
            "ProjectionExpression",
        )?;
        let pk = key_bytes(&t, &key)?;
        let found = self.read_item(&item_key(&t, &pk))?;
        let mut out = Map::new();
        if let Some(item) = &found {
            out.insert(
                "Item".into(),
                item_to_json(&narrowed(item, paths.as_deref())),
            );
        }
        let bytes = found.as_ref().map_or(1, item_size);
        let consistent = bool_of(&cx.body, "ConsistentRead").unwrap_or(false);
        if let Some(c) = consumed(&cx.body, &t.name, bytes, false, consistent) {
            out.insert("ConsumedCapacity".into(), c);
        }
        Ok(Json::Object(out))
    }

    pub(crate) fn delete_item(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let t = self.table(&cx.table_name("TableName")?)?;
        let key = item_member(&cx.body, "Key")?;
        let returning = return_values(&cx.body, &["NONE", "ALL_OLD"])?;
        let ex = Exprs::of(&cx.body)?;
        let mut scope = ex.scope();
        let cond = condition(&cx.body, &mut scope)?;
        finish(
            &scope,
            &used(&cx.body, &["ConditionExpression"]),
            "ConditionExpression",
        )?;
        let pk = key_bytes(&t, &key)?;
        let old = self.read_item(&item_key(&t, &pk))?;
        check(cond.as_ref(), old.as_ref(), &cx.body)?;
        if old.is_some() {
            self.stage(&t, cx.now_ms, old.as_ref(), None, false)?;
        }
        let mut out = Map::new();
        if returning == "ALL_OLD" {
            attributes(&mut out, old.as_ref());
        }
        capacity(&mut out, cx, &t, old.as_ref().map_or(1, item_size), true);
        Ok(Json::Object(out))
    }

    pub(crate) fn update_item(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let t = self.table(&cx.table_name("TableName")?)?;
        let key = item_member(&cx.body, "Key")?;
        let returning = return_values(
            &cx.body,
            &["NONE", "ALL_OLD", "UPDATED_OLD", "ALL_NEW", "UPDATED_NEW"],
        )?;
        let ex = Exprs::of(&cx.body)?;
        let mut scope = ex.scope();
        let update = match str_of(&cx.body, "UpdateExpression") {
            Some(text) => Some(parse_update(text, &mut scope).map_err(inv)?),
            None => None,
        };
        let cond = condition(&cx.body, &mut scope)?;
        let members = used(&cx.body, &["UpdateExpression", "ConditionExpression"]);
        finish(&scope, &members, "UpdateExpression")?;
        let pk = key_bytes(&t, &key)?;
        let old = self.read_item(&item_key(&t, &pk))?;
        check(cond.as_ref(), old.as_ref(), &cx.body)?;
        let mut new = old.clone().unwrap_or_else(|| key_of(&t, &key));
        if let Some(u) = &update {
            let keys: Vec<&str> = std::iter::once(t.hash.name.as_str())
                .chain(t.range.as_ref().map(|r| r.name.as_str()))
                .collect();
            u.apply(&mut new, &keys).map_err(inv)?;
        }
        validate_item(&t, &new)?;
        self.stage(&t, cx.now_ms, old.as_ref(), Some(&new), false)?;
        let mut out = Map::new();
        shown(&mut out, returning, old.as_ref(), &new, update.as_ref());
        capacity(&mut out, cx, &t, item_size(&new), true);
        Ok(Json::Object(out))
    }
}

/// The `Attributes` an update returns for each `ReturnValues`.
fn shown(
    out: &mut Map<String, Json>,
    returning: &str,
    old: Option<&Item>,
    new: &Item,
    update: Option<&Update>,
) {
    let touched = |with_remove: bool| -> Vec<aws_expr::Path> {
        let Some(u) = update else { return Vec::new() };
        let mut paths: Vec<aws_expr::Path> = u.set.iter().map(|(p, _)| p.clone()).collect();
        paths.extend(u.add.iter().map(|(p, _)| p.clone()));
        paths.extend(u.delete.iter().map(|(p, _)| p.clone()));
        if with_remove {
            paths.extend(u.remove.iter().cloned());
        }
        paths
    };
    match returning {
        "ALL_OLD" => attributes(out, old),
        "ALL_NEW" => attributes(out, Some(new)),
        "UPDATED_OLD" => {
            let part = old.map(|o| aws_expr::project(o, &touched(true)));
            attributes(out, part.as_ref().filter(|p| !p.is_empty()));
        }
        "UPDATED_NEW" => {
            let part = aws_expr::project(new, &touched(false));
            attributes(out, Some(&part).filter(|p| !p.is_empty()));
        }
        _ => {}
    }
}
