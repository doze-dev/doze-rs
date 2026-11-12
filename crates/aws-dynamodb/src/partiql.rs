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

use aws_wire::ApiError;
use serde_json::{Map, Value as Json, json};

use crate::core::{Core, Cx};
use crate::meta::Table;
use crate::req::{array_of, str_of, validation};

pub(crate) use parse::{Error as ParseError, Parsed, Statement};
use parse::{Expr, Func, Lit, Operand, Path, SetValue, Step};

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

/// Names and values for one classic request: `#n0`, `:v0`...
#[derive(Default)]
struct Placeholders {
    names: Map<String, Json>,
    values: Map<String, Json>,
}

impl Placeholders {
    fn name(&mut self, n: &str) -> String {
        if let Some((k, _)) = self.names.iter().find(|(_, v)| v.as_str() == Some(n)) {
            return k.clone();
        }
        let k = format!("#n{}", self.names.len());
        self.names.insert(k.clone(), json!(n));
        k
    }

    fn value(&mut self, v: Json) -> String {
        let k = format!(":v{}", self.values.len());
        self.values.insert(k.clone(), v);
        k
    }

    fn path(&mut self, p: &Path) -> String {
        let mut out = String::new();
        for step in &p.0 {
            match step {
                Step::Name(n) => {
                    if !out.is_empty() {
                        out.push('.');
                    }
                    let k = self.name(n);
                    out.push_str(&k);
                }
                Step::Index(i) => out.push_str(&format!("[{i}]")),
            }
        }
        out
    }

    /// Adds the maps to a request body (only those used: the classic operations refuse an unused one).
    fn into_body(self, body: &mut Json) {
        if !self.names.is_empty() {
            crate::req::set(body, "ExpressionAttributeNames", Json::Object(self.names));
        }
        if !self.values.is_empty() {
            crate::req::set(body, "ExpressionAttributeValues", Json::Object(self.values));
        }
    }
}

/// The statement's parameters, bound to its `?`s.
struct Params<'a>(&'a [Json]);

impl Params<'_> {
    /// A value as an attribute value.
    fn value(&self, lit: &Lit) -> Result<Json, ApiError> {
        Ok(match lit {
            Lit::Param(i) => self.0.get(*i).cloned().ok_or_else(|| {
                validation("Number of parameters in request and statement don't match.")
            })?,
            Lit::Str(s) => json!({"S": s}),
            Lit::Num(n) => json!({"N": n}),
            Lit::Bool(b) => json!({"BOOL": b}),
            Lit::Null => json!({"NULL": true}),
            Lit::List(items) => {
                json!({"L": items.iter().map(|i| self.value(i)).collect::<Result<Vec<_>, _>>()?})
            }
            Lit::Map(fields) => json!({"M": self.map(fields)?}),
            Lit::Bag(items) => self.set(items)?,
        })
    }

    fn map(&self, fields: &[(String, Lit)]) -> Result<Map<String, Json>, ApiError> {
        let mut m = Map::new();
        for (k, v) in fields {
            if m.insert(k.clone(), self.value(v)?).is_some() {
                return Err(validation(format!(
                    "Duplicate attribute name {k} in the item"
                )));
            }
        }
        Ok(m)
    }

    /// `<<...>>`: a string, number or binary set, all of one kind.
    fn set(&self, items: &[Lit]) -> Result<Json, ApiError> {
        let mut kind = None;
        let mut members = Vec::new();
        for i in items {
            let v = self.value(i)?;
            let (k, member) = ["S", "N", "B"]
                .iter()
                .find_map(|k| v.get(*k).map(|m| (*k, m.clone())))
                .ok_or_else(|| validation("A set holds strings, numbers or binaries"))?;
            if kind.is_some_and(|seen| seen != k) {
                return Err(validation("A set holds values of one type"));
            }
            kind = Some(k);
            members.push(member);
        }
        let key = format!("{}S", kind.unwrap_or("S"));
        Ok(json!({ key: members }))
    }

    fn operand(&self, ph: &mut Placeholders, o: &Operand) -> Result<String, ApiError> {
        Ok(match o {
            Operand::Path(p) => ph.path(p),
            Operand::Value(v) => ph.value(self.value(v)?),
            Operand::Size(p) => format!("size({})", ph.path(p)),
        })
    }

    /// A `WHERE` as a condition expression.
    fn condition(&self, ph: &mut Placeholders, e: &Expr) -> Result<String, ApiError> {
        Ok(match e {
            Expr::And(a, b) => format!(
                "({}) AND ({})",
                self.condition(ph, a)?,
                self.condition(ph, b)?
            ),
            Expr::Or(a, b) => format!(
                "({}) OR ({})",
                self.condition(ph, a)?,
                self.condition(ph, b)?
            ),
            Expr::Not(a) => format!("NOT ({})", self.condition(ph, a)?),
            Expr::Cmp(a, op, b) => format!(
                "{} {} {}",
                self.operand(ph, a)?,
                op.text(),
                self.operand(ph, b)?
            ),
            Expr::Between(a, lo, hi) => format!(
                "{} BETWEEN {} AND {}",
                self.operand(ph, a)?,
                self.operand(ph, lo)?,
                self.operand(ph, hi)?
            ),
            Expr::In(a, list) => {
                let items = list
                    .iter()
                    .map(|o| self.operand(ph, o))
                    .collect::<Result<Vec<_>, _>>()?;
                format!("{} IN ({})", self.operand(ph, a)?, items.join(", "))
            }
            Expr::Func(f, p, v) => {
                let name = match f {
                    Func::BeginsWith => "begins_with",
                    Func::Contains => "contains",
                    Func::AttributeType => "attribute_type",
                };
                format!("{name}({}, {})", ph.path(p), self.operand(ph, v)?)
            }
            Expr::Missing(p, true) => format!("attribute_not_exists({})", ph.path(p)),
            Expr::Missing(p, false) => format!("attribute_exists({})", ph.path(p)),
            Expr::Null(p, is) => {
                let path = ph.path(p);
                let null = ph.value(json!({"S": "NULL"}));
                let test =
                    format!("(attribute_not_exists({path}) OR attribute_type({path}, {null}))");
                if *is { test } else { format!("NOT {test}") }
            }
        })
    }
}

/// The parts of a `WHERE` joined by `AND` at its top.
fn conjuncts(e: &Expr) -> Vec<&Expr> {
    match e {
        Expr::And(a, b) => {
            let mut out = conjuncts(a);
            out.extend(conjuncts(b));
            out
        }
        other => vec![other],
    }
}

/// The value a conjunct pins an attribute to, if it is `attribute = value` (either way round).
fn pinned<'e>(e: &'e Expr, attribute: &str) -> Option<&'e Lit> {
    match e {
        Expr::Cmp(Operand::Path(p), parse::Cmp::Eq, Operand::Value(v))
        | Expr::Cmp(Operand::Value(v), parse::Cmp::Eq, Operand::Path(p))
            if p.attribute() == Some(attribute) =>
        {
            Some(v)
        }
        _ => None,
    }
}

/// The key attributes a `WHERE` pins, each to its value.
type Key<'e> = Vec<(&'e str, &'e Lit)>;

/// The key a `WHERE` names, and what else it says. `None` when it does not pin every key attribute.
fn split_key<'e>(t: &'e Table, filter: &'e Expr) -> Option<(Key<'e>, Vec<&'e Expr>)> {
    let mut rest = conjuncts(filter);
    let mut key = Vec::new();
    for k in std::iter::once(&t.hash).chain(t.range.as_ref()) {
        let at = rest.iter().position(|e| pinned(e, &k.name).is_some())?;
        let e = rest.remove(at);
        key.push((k.name.as_str(), pinned(e, &k.name)?));
    }
    Some((key, rest))
}

fn no_key() -> ApiError {
    validation("Where clause does not contain a mandatory equality on all key attributes")
}

/// A classic request with the caller's options carried over.
fn carry(body: &mut Json, from: &Json, members: &[&str]) {
    for m in members {
        if let Some(v) = from.get(*m) {
            crate::req::set(body, m, v.clone());
        }
    }
}

/// What a statement became: the classic request, and how to read its answer.
enum Plan {
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
    fn set(&mut self, member: &str, value: Json) {
        crate::req::set(&mut self.body, member, value);
    }

    fn finish(mut self, carried: &[&str]) -> Json {
        carry(&mut self.body, self.options, carried);
        self.ph.into_body(&mut self.body);
        self.body
    }

    fn key(&mut self, key: &[(&str, &Lit)]) -> Result<(), ApiError> {
        let mut k = Map::new();
        for (name, v) in key {
            k.insert((*name).to_owned(), self.params.value(v)?);
        }
        self.set("Key", Json::Object(k));
        Ok(())
    }

    /// `parts` and the conditions of `rest`, all to hold.
    fn condition(&mut self, mut parts: Vec<String>, rest: &[&Expr]) -> Result<(), ApiError> {
        for e in rest {
            parts.push(self.params.condition(&mut self.ph, e)?);
        }
        if !parts.is_empty() {
            self.set("ConditionExpression", json!(join_and(&parts)));
        }
        Ok(())
    }

    fn returning(&mut self, returning: Option<parse::Returning>) {
        if let Some(r) = returning {
            self.set("ReturnValues", json!(r.return_values()));
        }
    }

    fn select(
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

    fn insert(mut self, item: &[(String, Lit)]) -> Result<Plan, ApiError> {
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
    fn assignment(
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
    fn update(
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

    fn delete(
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
    fn plan(&self, p: &Parsed, params: &[Json], options: &Json) -> Result<Plan, ApiError> {
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

fn join_and(parts: &[String]) -> String {
    parts
        .iter()
        .map(|p| format!("({p})"))
        .collect::<Vec<_>>()
        .join(" AND ")
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

fn read_token(token: &str) -> Result<Json, ApiError> {
    aws_core::base64::decode(token)
        .and_then(|b| serde_json::from_slice::<Json>(&b).ok())
        .filter(Json::is_object)
        .ok_or_else(|| validation("Invalid NextToken"))
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
