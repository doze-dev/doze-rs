//! TransactGetItems and TransactWriteItems: all or nothing, with AWS's reasons when it is nothing.
//!
//! Wording: the messages in `conformance/snapshots/test_dynamodb.json` (recorded 2026-11-09) are AWS's own; every other message in this file is
//! `(memory)`: from the developer guide and from use, and unchecked until a recording reaches it.

use std::collections::BTreeSet;

use aws_expr::{Cond, Item, Update, item_size, item_to_json, parse_update};
use aws_wire::ApiError;
use serde_json::{Value as Json, json};

use crate::condition::{condition, item_member, narrowed, projection, used};
use crate::core::{Core, Cx};
use crate::meta::Table;
use crate::req::{Exprs, array_of, finish, inv, str_of, table_name, validation};
use crate::stage::{item_key, key_bytes, key_of, validate_item};

const MAX_ACTIONS: usize = 100;
/// The most item data one transaction may carry. The text of the refusal is `(memory)`.
const MAX_BYTES: usize = 4 * 1024 * 1024;
/// How long a client request token keeps a transaction from being applied twice.
const TOKEN_MS: i64 = 10 * 60 * 1000;

/// What one action of a transaction does.
#[derive(Debug)]
enum Act {
    Check,
    Put(Item),
    Delete,
    Update(Item, Option<Update>),
}

/// An action, checked and ready.
#[derive(Debug)]
struct Step {
    t: Table,
    pk: Vec<u8>,
    act: Act,
    cond: Option<Cond>,
    /// The request's `ReturnValuesOnConditionCheckFailure` was `ALL_OLD`.
    want_item: bool,
}

impl Core {
    pub(crate) fn transact_get(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let actions = array_of(&cx.body, "TransactItems");
        if actions.len() > MAX_ACTIONS {
            return Err(too_many(actions.len()));
        }
        let mut responses = Vec::new();
        for a in actions {
            let get = a.get("Get").cloned().unwrap_or_default();
            let t = self.table(table_name(
                str_of(&get, "TableName").unwrap_or_default(),
                cx.region(),
                cx.account(),
            )?)?;
            let ex = Exprs::of(&get)?;
            let mut scope = ex.scope();
            let paths = projection(&get, &mut scope)?;
            finish(
                &scope,
                &used(&get, &["ProjectionExpression"]),
                "ProjectionExpression",
            )?;
            let pk = key_bytes(&t, &item_member(&get, "Key")?)?;
            responses.push(match self.read_item(&item_key(&t, &pk))? {
                Some(item) => json!({"Item": item_to_json(&narrowed(&item, paths.as_deref()))}),
                None => json!({}),
            });
        }
        Ok(json!({ "Responses": responses }))
    }

    pub(crate) fn transact_write(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let actions = array_of(&cx.body, "TransactItems");
        if actions.len() > MAX_ACTIONS {
            return Err(too_many(actions.len()));
        }
        let mut steps = Vec::new();
        for a in actions {
            steps.push(self.step(cx, a)?);
        }
        let mut seen = BTreeSet::new();
        if !steps
            .iter()
            .all(|s| seen.insert((s.t.name.clone(), s.pk.clone())))
        {
            return Err(validation(
                "Transaction request cannot include multiple operations on one item",
            ));
        }
        if self.replayed(cx)? {
            return Ok(Json::Null);
        }
        let olds = self.reasons(&steps)?;
        let mut bytes = 0;
        for (step, old) in steps.iter().zip(&olds) {
            bytes += self.apply(cx, step, old.as_ref())?;
        }
        if bytes > MAX_BYTES {
            return Err(validation(
                "Transaction request cannot include more than 4 MB of items",
            ));
        }
        self.remember(cx);
        Ok(Json::Null)
    }

    /// Reads and checks one action.
    fn step(&self, cx: &Cx<'_>, action: &Json) -> Result<Step, ApiError> {
        let (kind, a) = ["ConditionCheck", "Put", "Delete", "Update"]
            .iter()
            .find_map(|k| action.get(*k).map(|v| (*k, v)))
            .ok_or_else(|| {
                validation(
                    "TransactItems can only contain one of ConditionCheck, Put, Delete or Update",
                )
            })?;
        let t = self.table(table_name(
            str_of(a, "TableName").unwrap_or_default(),
            cx.region(),
            cx.account(),
        )?)?;
        let ex = Exprs::of(a)?;
        let mut scope = ex.scope();
        let update = match (kind, str_of(a, "UpdateExpression")) {
            ("Update", Some(text)) => Some(parse_update(text, &mut scope).map_err(inv)?),
            _ => None,
        };
        let cond = condition(a, &mut scope)?;
        finish(
            &scope,
            &used(a, &["UpdateExpression", "ConditionExpression"]),
            "UpdateExpression",
        )?;
        let want_item = str_of(a, "ReturnValuesOnConditionCheckFailure") == Some("ALL_OLD");
        let (pk, act) = match kind {
            "Put" => {
                let item = item_member(a, "Item")?;
                (validate_item(&t, &item)?, Act::Put(item))
            }
            "Delete" => (key_bytes(&t, &item_member(a, "Key")?)?, Act::Delete),
            "Update" => {
                let key = item_member(a, "Key")?;
                (key_bytes(&t, &key)?, Act::Update(key_of(&t, &key), update))
            }
            _ => (key_bytes(&t, &item_member(a, "Key")?)?, Act::Check),
        };
        Ok(Step {
            t,
            pk,
            act,
            cond,
            want_item,
        })
    }

    /// Reads each item and checks each condition; if any fails, the transaction is cancelled and says why for each action.
    fn reasons(&self, steps: &[Step]) -> Result<Vec<Option<Item>>, ApiError> {
        let mut olds = Vec::new();
        let mut reasons = Vec::new();
        let mut failed = false;
        for s in steps {
            let old = self.read_item(&item_key(&s.t, &s.pk))?;
            let ok = s
                .cond
                .as_ref()
                .is_none_or(|c| c.eval(old.as_ref().unwrap_or(&Item::new())));
            failed |= !ok;
            reasons.push(if ok {
                json!({"Code": "None"})
            } else {
                let mut r = json!({"Code": "ConditionalCheckFailed", "Message": "The conditional request failed"});
                if let (true, Some(item)) = (s.want_item, &old) {
                    crate::req::set(&mut r, "Item", item_to_json(item));
                }
                r
            });
            olds.push(old);
        }
        if failed {
            return Err(cancelled(&reasons));
        }
        Ok(olds)
    }

    /// Applies one action of a transaction that passed, and says how many bytes of item it wrote.
    fn apply(&mut self, cx: &Cx<'_>, s: &Step, old: Option<&Item>) -> Result<usize, ApiError> {
        match &s.act {
            Act::Check => Ok(0),
            Act::Put(item) => self
                .stage(&s.t, cx.now_ms, old, Some(item), false)
                .map(|()| item_size(item)),
            Act::Delete if old.is_some() => {
                self.stage(&s.t, cx.now_ms, old, None, false).map(|()| 0)
            }
            Act::Delete => Ok(0),
            Act::Update(key, update) => {
                let mut new = old.cloned().unwrap_or_else(|| key.clone());
                if let Some(u) = update {
                    let names: Vec<&str> = std::iter::once(s.t.hash.name.as_str())
                        .chain(s.t.range.as_ref().map(|r| r.name.as_str()))
                        .collect();
                    u.apply(&mut new, &names).map_err(inv)?;
                }
                validate_item(&s.t, &new)?;
                self.stage(&s.t, cx.now_ms, old, Some(&new), false)
                    .map(|()| item_size(&new))
            }
        }
    }

    /// Whether this request carries a client token that already ran: the same request is answered as done, a different one is
    /// refused. A token counts for ten minutes.
    fn replayed(&mut self, cx: &Cx<'_>) -> Result<bool, ApiError> {
        let Some(token) = str_of(&cx.body, "ClientRequestToken") else {
            return Ok(false);
        };
        self.tokens.retain(|_, (_, until)| *until > cx.now_ms);
        let print = fingerprint(&cx.body);
        match self.tokens.get(token) {
            Some((seen, _)) if *seen == print => Ok(true),
            Some(_) => Err(ApiError::modeled(
                "IdempotentParameterMismatchException",
                "The request uses the same client token as a previous, but non-identical request.",
            )),
            None => Ok(false),
        }
    }

    /// Notes that a transaction with a client token went through.
    fn remember(&mut self, cx: &Cx<'_>) {
        if let Some(token) = str_of(&cx.body, "ClientRequestToken") {
            self.tokens.insert(
                token.to_owned(),
                (fingerprint(&cx.body), cx.now_ms + TOKEN_MS),
            );
        }
    }
}

/// A fingerprint of a request without its token and capacity options.
fn fingerprint(body: &Json) -> u64 {
    let mut copy = body.clone();
    if let Json::Object(m) = &mut copy {
        m.remove("ClientRequestToken");
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in copy.to_string().bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn too_many(n: usize) -> ApiError {
    validation(format!(
        "1 validation error detected: Value '[{n} items]' at 'transactItems' failed to satisfy constraint: Member must have length less than or equal to {MAX_ACTIONS}"
    ))
}

/// `TransactionCanceledException`, with the reason for each action in the message and in `CancellationReasons`.
fn cancelled(reasons: &[Json]) -> ApiError {
    let codes: Vec<&str> = reasons
        .iter()
        .filter_map(|r| r.get("Code").and_then(Json::as_str))
        .collect();
    ApiError::modeled(
        "TransactionCanceledException",
        format!(
            "Transaction cancelled, please refer cancellation reasons for specific reasons [{}]",
            codes.join(", ")
        ),
    )
    .with_body(
        "CancellationReasons",
        Json::Array(reasons.to_vec()).to_string(),
    )
}
