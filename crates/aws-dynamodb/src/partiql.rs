//! PartiQL: `ExecuteStatement`, `BatchExecuteStatement` and `ExecuteTransaction`. A statement is read ([`parse`]) and translated onto
//! the classic operation it means, which then runs as if it had been called, so PartiQL shares their storage, key checks, conditions,
//! streams and refusals:
//!
//! - `SELECT` whose `WHERE` is exactly an equality on every key attribute is a `GetItem`; any other `SELECT` is a `Scan` (of the
//!   table, or of the index named `"table"."index"`) with the `WHERE` as its filter. `Limit` and `NextToken` page it.
//! - `INSERT` is a `PutItem` that may not replace an item (`DuplicateItemException`).
//! - `UPDATE` and `DELETE` need an equality on every key attribute; the rest of the `WHERE` is the condition. An `UPDATE` does not
//!   make an item that is not there (the condition fails).
//! - `ExecuteTransaction` is a `TransactWriteItems` (all writes) or a `TransactGetItems` (all full-key reads), so it is all or nothing.
//! - `BatchExecuteStatement` runs each statement on its own and answers for each.
//!
//! What is not built is refused by name (501 `NotImplemented`). The messages are `(memory)` unless a recording says otherwise.

mod parse;
mod plan;
mod translate;

use aws_wire::ApiError;
use serde_json::{Value as Json, json};

use crate::core::{Core, Cx};

use crate::req::{array_of, str_of, validation};

pub(crate) use parse::{Error as ParseError, Statement};
use plan::Plan;
use translate::carry;

/// The most statements a batch carries (the model's limit).
const MAX_BATCH: usize = 25;
/// The most statements a transaction carries (the model's limit).
const MAX_TRANSACTION: usize = 100;

/// Reads a statement and answers whether it is one this build runs (the fuzz target's entry point).
///
/// # Errors
/// Why it is refused.
pub fn check(text: &str) -> Result<(), String> {
    parse::parse(text).map(|_| ()).map_err(|e| e.to_string())
}

/// A refusal of a statement, as the caller sees it.
fn refused(e: &ParseError) -> ApiError {
    match e {
        ParseError::Syntax(_) => validation(e.to_string()),
        ParseError::NotBuilt(_) => ApiError::new(501, "NotImplemented", e.to_string()),
    }
}

/// The table (or `table/index/name`) and IAM action of a statement, for the authorizer.
pub(crate) fn action_of(text: &str) -> Option<(&'static str, String, Option<String>)> {
    let p = parse::parse(text).ok()?;
    let (action, index) = match &p.statement {
        Statement::Select { index, .. } => ("PartiQLSelect", index.clone()),
        Statement::Insert { .. } => ("PartiQLInsert", None),
        Statement::Update { .. } => ("PartiQLUpdate", None),
        Statement::Delete { .. } => ("PartiQLDelete", None),
    };
    Some((action, p.statement.table().to_owned(), index))
}

impl Core {
    /// Runs a plan as the classic operation it is, and answers as `ExecuteStatement` does.
    fn run_plan(&mut self, cx: &Cx<'_>, plan: Plan) -> Result<Json, ApiError> {
        let sub = |body: Json| Cx {
            call: cx.call,
            body,
            now_ms: cx.now_ms,
        };
        match plan {
            Plan::Get(body) => {
                let out = self.get_item(&sub(body))?;
                let items: Vec<Json> = out.get("Item").cloned().into_iter().collect();
                Ok(with_capacity(json!({ "Items": items }), &out))
            }
            Plan::Scan(body) => {
                let out = self.scan(&sub(body))?;
                let mut answer =
                    json!({ "Items": out.get("Items").cloned().unwrap_or_else(|| json!([])) });
                if let Some(last) = out.get("LastEvaluatedKey") {
                    crate::req::set(&mut answer, "NextToken", json!(write_token(last)));
                    crate::req::set(&mut answer, "LastEvaluatedKey", last.clone());
                }
                Ok(with_capacity(answer, &out))
            }
            Plan::Put(body) => match self.put_item(&sub(body)) {
                Ok(out) => Ok(with_capacity(json!({ "Items": [] }), &out)),
                Err(e) if e.code == "ConditionalCheckFailedException" => Err(duplicate()),
                Err(e) => Err(e),
            },
            Plan::Update(body) => {
                let out = self.update_item(&sub(body))?;
                Ok(with_capacity(returned(&out), &out))
            }
            Plan::Delete(body) => {
                let out = self.delete_item(&sub(body))?;
                Ok(with_capacity(returned(&out), &out))
            }
        }
    }

    /// The table and key a write names, as text (for finding two writes of one item).
    fn target(&self, plan: &Plan) -> Option<(String, String)> {
        let (body, member) = match plan {
            Plan::Put(b) => (b, "Item"),
            Plan::Update(b) | Plan::Delete(b) => (b, "Key"),
            Plan::Get(_) | Plan::Scan(_) => return None,
        };
        let name = str_of(body, "TableName")?;
        let t = self.tables.get(name)?;
        let item = body.get(member)?;
        let key: Vec<String> = std::iter::once(&t.hash)
            .chain(t.range.as_ref())
            .map(|k| item.get(&k.name).map(Json::to_string).unwrap_or_default())
            .collect();
        Some((name.to_owned(), key.join("\u{0}")))
    }

    fn statement_of(&self, item: &Json, options: &Json) -> Result<Plan, ApiError> {
        let text = str_of(item, "Statement").unwrap_or_default();
        let parsed = parse::parse(text).map_err(|e| refused(&e))?;
        let params = array_of(item, "Parameters");
        self.plan(&parsed, params, options)
    }

    pub(crate) fn execute_statement(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let plan = self.statement_of(&cx.body, &cx.body)?;
        self.run_plan(cx, plan)
    }

    /// Each statement on its own: a failure is that statement's answer, not the batch's. All reads or all writes, and a read names
    /// a whole key.
    pub(crate) fn batch_execute_statement(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let statements = array_of(&cx.body, "Statements");
        if statements.len() > MAX_BATCH {
            return Err(validation(format!(
                "1 validation error detected: Value at 'statements' failed to satisfy constraint: Member must have length less than or equal to {MAX_BATCH}"
            )));
        }
        let mut plans = Vec::with_capacity(statements.len());
        for s in statements {
            plans.push(self.statement_of(s, s));
        }
        let reads = plans
            .iter()
            .filter(|p| matches!(p, Ok(Plan::Get(_) | Plan::Scan(_))))
            .count();
        let writes = plans
            .iter()
            .filter(|p| matches!(p, Ok(Plan::Put(_) | Plan::Update(_) | Plan::Delete(_))))
            .count();
        if reads > 0 && writes > 0 {
            return Err(validation(
                "Batch statements must be either all reads or all writes",
            ));
        }
        // Each statement reads the table as it was before the batch, so two writes of one item would not see each other.
        let mut keys = std::collections::BTreeSet::new();
        for plan in plans.iter().flatten() {
            if let Some(k) = self.target(plan)
                && !keys.insert(k)
            {
                return Err(validation("Provided list of item keys contains duplicates"));
            }
        }
        let mut responses = Vec::with_capacity(plans.len());
        for (s, plan) in statements.iter().zip(plans) {
            let table = parse::parse(str_of(s, "Statement").unwrap_or_default())
                .ok()
                .map(|p| p.statement.table().to_owned());
            let answer = match plan {
                Ok(Plan::Scan(_)) => Err(validation(
                    "A read in a batch must name a whole key with equalities",
                )),
                Ok(plan) => self.run_plan(cx, plan),
                Err(e) => Err(e),
            };
            let mut r = json!({});
            if let Some(t) = &table {
                crate::req::set(&mut r, "TableName", json!(t));
            }
            match answer {
                Ok(out) => {
                    if let Some(item) = out.get("Items").and_then(|i| i.get(0)) {
                        crate::req::set(&mut r, "Item", item.clone());
                    }
                }
                Err(e) => {
                    let mut err = json!({"Code": batch_code(&e.code), "Message": e.message});
                    if let Some(item) = body_field(&e, "Item") {
                        crate::req::set(&mut err, "Item", item);
                    }
                    crate::req::set(&mut r, "Error", err);
                }
            }
            responses.push(r);
        }
        Ok(json!({ "Responses": responses }))
    }

    /// All writes, as one `TransactWriteItems`, or all whole-key reads, as one `TransactGetItems`.
    pub(crate) fn execute_transaction(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let statements = array_of(&cx.body, "TransactStatements");
        if statements.len() > MAX_TRANSACTION {
            return Err(validation(format!(
                "1 validation error detected: Value at 'transactStatements' failed to satisfy constraint: Member must have length less than or equal to {MAX_TRANSACTION}"
            )));
        }
        let mut actions = Vec::with_capacity(statements.len());
        let mut inserts = Vec::new();
        let (mut reads, mut writes) = (0, 0);
        for (i, s) in statements.iter().enumerate() {
            let action = match self.statement_of(s, s)? {
                Plan::Get(body) => {
                    reads += 1;
                    json!({"Get": body})
                }
                Plan::Scan(_) => {
                    return Err(validation(
                        "A read in a transaction must name a whole key with equalities",
                    ));
                }
                Plan::Put(body) => {
                    writes += 1;
                    inserts.push(i);
                    json!({"Put": body})
                }
                Plan::Update(body) => {
                    writes += 1;
                    json!({"Update": without(body, "ReturnValues")})
                }
                Plan::Delete(body) => {
                    writes += 1;
                    json!({"Delete": without(body, "ReturnValues")})
                }
            };
            actions.push(action);
        }
        if reads > 0 && writes > 0 {
            return Err(validation(
                "Transaction statements must be either all reads or all writes",
            ));
        }
        let mut body = json!({ "TransactItems": actions });
        carry(
            &mut body,
            &cx.body,
            &["ClientRequestToken", "ReturnConsumedCapacity"],
        );
        let sub = Cx {
            call: cx.call,
            body,
            now_ms: cx.now_ms,
        };
        if reads > 0 {
            return self.transact_get(&sub);
        }
        match self.transact_write(&sub) {
            Ok(_) => Ok(json!({})),
            Err(e) => Err(duplicates_named(e, &inserts)),
        }
    }
}

/// `TransactionCanceledException`: an `INSERT` that found its key taken is a `DuplicateItem`, not a failed condition `(memory)`.
fn duplicates_named(mut e: ApiError, inserts: &[usize]) -> ApiError {
    if e.code != "TransactionCanceledException" {
        return e;
    }
    let Some(fields) = e.body.as_mut() else {
        return e;
    };
    let mut codes = Vec::new();
    for (name, value) in &mut fields.0 {
        if name != "CancellationReasons" {
            continue;
        }
        let Ok(Json::Array(mut reasons)) = serde_json::from_str::<Json>(value) else {
            continue;
        };
        for (i, r) in reasons.iter_mut().enumerate() {
            if inserts.contains(&i)
                && r.get("Code").and_then(Json::as_str) == Some("ConditionalCheckFailed")
            {
                crate::req::set(r, "Code", json!("DuplicateItem"));
                crate::req::set(r, "Message", json!("Duplicate primary key exists in table"));
            }
            codes.push(
                r.get("Code")
                    .and_then(Json::as_str)
                    .unwrap_or("None")
                    .to_owned(),
            );
        }
        *value = Json::Array(reasons).to_string();
    }
    if !codes.is_empty() {
        e.message = format!(
            "Transaction cancelled, please refer cancellation reasons for specific reasons [{}]",
            codes.join(", ")
        );
    }
    e
}

fn without(mut body: Json, member: &str) -> Json {
    if let Json::Object(m) = &mut body {
        m.remove(member);
    }
    body
}

fn duplicate() -> ApiError {
    ApiError::modeled(
        "DuplicateItemException",
        "Duplicate primary key exists in table",
    )
}

/// `RETURNING`: the attributes an update or delete gave back, as the one item of `Items`.
fn returned(out: &Json) -> Json {
    let items: Vec<Json> = out.get("Attributes").cloned().into_iter().collect();
    json!({ "Items": items })
}

fn with_capacity(mut answer: Json, out: &Json) -> Json {
    if let Some(c) = out.get("ConsumedCapacity") {
        crate::req::set(&mut answer, "ConsumedCapacity", c.clone());
    }
    answer
}

/// `NextToken`: the scan's `LastEvaluatedKey`, opaque.
fn write_token(last: &Json) -> String {
    aws_core::base64::encode(last.to_string().as_bytes())
}

/// An error's code as a batch answers it (`BatchStatementErrorCodeEnum`).
fn batch_code(code: &str) -> &'static str {
    match code {
        "ConditionalCheckFailedException" => "ConditionalCheckFailed",
        "ResourceNotFoundException" => "ResourceNotFound",
        "DuplicateItemException" => "DuplicateItem",
        "ItemCollectionSizeLimitExceededException" => "ItemCollectionSizeLimitExceeded",
        "TransactionConflictException" => "TransactionConflict",
        "ProvisionedThroughputExceededException" => "ProvisionedThroughputExceeded",
        "AccessDeniedException" => "AccessDenied",
        "InternalServerError" => "InternalServerError",
        _ => "ValidationError",
    }
}

fn body_field(e: &ApiError, name: &str) -> Option<Json> {
    e.body
        .as_ref()?
        .0
        .iter()
        .find(|(n, _)| n == name)
        .and_then(|(_, v)| serde_json::from_str(v).ok())
}
