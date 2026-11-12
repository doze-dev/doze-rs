//! The classic request a statement means.

use aws_wire::ApiError;
use serde_json::{Map, Value as Json, json};

use super::parse::{self, Expr, Lit, Parsed, Path, SetValue, Statement};
use super::translate::{Params, Placeholders, carry, join_and, no_key, read_token, split_key};
use crate::core::Core;
use crate::meta::Table;
use crate::req::{str_of, validation};

/// What a statement became: the classic request, and how to read its answer.
pub(super) enum Plan {
    Get(Json),
    Scan(Json),
    Put(Json),
    Update(Json),
    Delete(Json),
}

/// One classic request being written from a statement.
struct Builder<'a> {
    t: &'a Table,
    params: Params<'a>,
    /// The request member that carried the statement: it may say `ConsistentRead`, `Limit`, `NextToken`,
    /// `ReturnValuesOnConditionCheckFailure`, `ReturnConsumedCapacity`.
    options: &'a Json,
    ph: Placeholders,
    body: Json,
}

impl Builder<'_> {
    pub(super) fn set(&mut self, member: &str, value: Json) {
        crate::req::set(&mut self.body, member, value);
    }

    pub(super) fn finish(mut self, carried: &[&str]) -> Json {
        carry(&mut self.body, self.options, carried);
        self.ph.into_body(&mut self.body);
        self.body
    }

    pub(super) fn key(&mut self, key: &[(&str, &Lit)]) -> Result<(), ApiError> {
        let mut k = Map::new();
        for (name, v) in key {
            k.insert((*name).to_owned(), self.params.value(v)?);
        }
        self.set("Key", Json::Object(k));
        Ok(())
    }

    /// `parts` and the conditions of `rest`, all to hold.
    pub(super) fn condition(
        &mut self,
        mut parts: Vec<String>,
        rest: &[&Expr],
    ) -> Result<(), ApiError> {
        for e in rest {
            parts.push(self.params.condition(&mut self.ph, e)?);
        }
        if !parts.is_empty() {
            self.set("ConditionExpression", json!(join_and(&parts)));
        }
        Ok(())
    }

    pub(super) fn returning(&mut self, returning: Option<parse::Returning>) {
        if let Some(r) = returning {
            self.set("ReturnValues", json!(r.return_values()));
        }
    }

    pub(super) fn select(
        mut self,
        index: Option<&String>,
        columns: Option<&Vec<Path>>,
        filter: Option<&Expr>,
    ) -> Result<Plan, ApiError> {
        if let Some(cols) = columns {
            let proj: Vec<String> = cols.iter().map(|c| self.ph.path(c)).collect();
            self.set("ProjectionExpression", json!(proj.join(", ")));
        }
        let t = self.t;
        let whole_key = filter
            .and_then(|f| split_key(t, f))
            .filter(|(_, rest)| rest.is_empty() && index.is_none());
        if let Some((key, _)) = whole_key {
            self.key(&key)?;
            return Ok(Plan::Get(
                self.finish(&["ConsistentRead", "ReturnConsumedCapacity"]),
            ));
        }
        if let Some(i) = index {
            if t.index(i).is_none() {
                return Err(crate::req::not_found());
            }
            self.set("IndexName", json!(i));
        }
        if let Some(f) = filter {
            let text = self.params.condition(&mut self.ph, f)?;
            self.set("FilterExpression", json!(text));
        }
        if let Some(token) = str_of(self.options, "NextToken") {
            self.set("ExclusiveStartKey", read_token(token)?);
        }
        Ok(Plan::Scan(self.finish(&[
            "ConsistentRead",
            "Limit",
            "ReturnConsumedCapacity",
        ])))
    }

    pub(super) fn insert(mut self, item: &[(String, Lit)]) -> Result<Plan, ApiError> {
        let item = self.params.map(item)?;
        self.set("Item", Json::Object(item));
        let hash = self.ph.name(&self.t.hash.name);
        self.set(
            "ConditionExpression",
            json!(format!("attribute_not_exists({hash})")),
        );
        Ok(Plan::Put(self.finish(&["ReturnConsumedCapacity"])))
    }

    /// One `SET`'s part of the update expression: `(clause, text)`.
    pub(super) fn assignment(
        &mut self,
        path: &Path,
        value: &SetValue,
    ) -> Result<(&'static str, String), ApiError> {
        let target = self.ph.path(path);
        let (p, ph) = (&self.params, &mut self.ph);
        Ok(match value {
            SetValue::Operand(o) => ("SET", format!("{target} = {}", p.operand(ph, o)?)),
            SetValue::Plus(a, b) => {
                let (a, b) = (p.operand(ph, a)?, p.operand(ph, b)?);
                ("SET", format!("{target} = {a} + {b}"))
            }
            SetValue::Minus(a, b) => {
                let (a, b) = (p.operand(ph, a)?, p.operand(ph, b)?);
                ("SET", format!("{target} = {a} - {b}"))
            }
            SetValue::ListAppend(a, b) => {
                let (a, b) = (p.operand(ph, a)?, p.operand(ph, b)?);
                ("SET", format!("{target} = list_append({a}, {b})"))
            }
            SetValue::IfNotExists(path, v) => {
                let (path, v) = (ph.path(path), p.operand(ph, v)?);
                ("SET", format!("{target} = if_not_exists({path}, {v})"))
            }
            SetValue::SetAdd(_, v) => ("ADD", format!("{target} {}", p.operand(ph, v)?)),
            SetValue::SetDelete(_, v) => ("DELETE", format!("{target} {}", p.operand(ph, v)?)),
        })
    }

    /// An update that may not make an item: the item must be there, and the rest of the `WHERE` hold.
    pub(super) fn update(
        mut self,
        set: &[(Path, SetValue)],
        remove: &[Path],
        filter: &Expr,
        returning: Option<parse::Returning>,
    ) -> Result<Plan, ApiError> {
        let mut clauses: Vec<(&str, Vec<String>)> = ["SET", "REMOVE", "ADD", "DELETE"]
            .into_iter()
            .map(|w| (w, Vec::new()))
            .collect();
        let mut add = |word: &str, part: String| {
            if let Some((_, parts)) = clauses.iter_mut().find(|(w, _)| *w == word) {
                parts.push(part);
            }
        };
        for (path, value) in set {
            let (word, part) = self.assignment(path, value)?;
            add(word, part);
        }
        for path in remove {
            add("REMOVE", self.ph.path(path));
        }
        let text: Vec<String> = clauses
            .iter()
            .filter(|(_, parts)| !parts.is_empty())
            .map(|(word, parts)| format!("{word} {}", parts.join(", ")))
            .collect();
        self.set("UpdateExpression", json!(text.join(" ")));
        let t = self.t;
        let (key, rest) = split_key(t, filter).ok_or_else(no_key)?;
        self.key(&key)?;
        let exists = format!("attribute_exists({})", self.ph.name(&t.hash.name));
        self.condition(vec![exists], &rest)?;
        self.returning(returning);
        Ok(Plan::Update(self.finish(&[
            "ReturnValuesOnConditionCheckFailure",
            "ReturnConsumedCapacity",
        ])))
    }

    pub(super) fn delete(
        mut self,
        filter: &Expr,
        returning: Option<parse::Returning>,
    ) -> Result<Plan, ApiError> {
        let (key, rest) = split_key(self.t, filter).ok_or_else(no_key)?;
        self.key(&key)?;
        self.condition(Vec::new(), &rest)?;
        self.returning(returning);
        Ok(Plan::Delete(self.finish(&[
            "ReturnValuesOnConditionCheckFailure",
            "ReturnConsumedCapacity",
        ])))
    }
}

impl Core {
    /// The classic request a statement means. `options` is the request member that carries the statement.
    pub(super) fn plan(
        &self,
        p: &Parsed,
        params: &[Json],
        options: &Json,
    ) -> Result<Plan, ApiError> {
        if params.len() != p.params {
            return Err(validation(
                "Number of parameters in request and statement don't match.",
            ));
        }
        let t = self.table(p.statement.table())?;
        let b = Builder {
            t: &t,
            params: Params(params),
            options,
            ph: Placeholders::default(),
            body: json!({"TableName": t.name}),
        };
        match &p.statement {
            Statement::Select {
                index,
                columns,
                filter,
                ..
            } => b.select(index.as_ref(), columns.as_ref(), filter.as_ref()),
            Statement::Insert { item, .. } => b.insert(item),
            Statement::Update {
                set,
                remove,
                filter,
                returning,
                ..
            } => b.update(set, remove, filter, *returning),
            Statement::Delete {
                filter, returning, ..
            } => b.delete(filter, *returning),
        }
    }
}
