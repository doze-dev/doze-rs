//! `DescribeEvents`: the stack events of one operation, a stack, or the operation that executed a change set, each with what its
//! operation was and how it stands (the model's `OperationEvent`). Built on the stack events; what each operation was is kept in a
//! table of its own (`operations`), written when an operation's first event is.
//!
//! Not recorded against AWS: which `EventType` each event takes (here `STACK_EVENT` for the stack's own events,
//! `PROVISIONING_ERROR` for a resource that failed, `PROGRESS_EVENT` for the rest), and what `StartTime` and `EndTime` hold (here
//! when the resource's work in the operation began, and when it ended, on the event that ends it). doze-aws runs no hooks and no
//! early validations, so there are no hook or validation events.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use aws_wire::ApiError;

use crate::core::Core;
use crate::describe::secs;
use crate::errors::validation;
use crate::j::J;
use crate::model::Event;
use crate::req::Req;

/// How many events a page holds.
const PAGE: usize = 100;

/// One operation on a stack.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Operation {
    pub(crate) stack_id: String,
    /// `CREATE_STACK`, `UPDATE_STACK`, `DELETE_STACK`, `ROLLBACK` or `CONTINUE_ROLLBACK`.
    pub(crate) kind: String,
    /// The change set it executed.
    pub(crate) change_set: Option<String>,
}

/// The model's operation type for what the stack record calls its last operation.
fn kind_of(last_op: &str) -> &str {
    match last_op {
        "ROLLBACK_STACK" => "ROLLBACK",
        other => other,
    }
}

/// What an operation is, and how it stands.
#[derive(Debug, Default)]
struct Op {
    kind: String,
    status: &'static str,
    /// When each resource's work in it began.
    started: BTreeMap<String, i64>,
}

fn op_status(kind: &str, last: &str) -> &'static str {
    if last.ends_with("_IN_PROGRESS") {
        "IN_PROGRESS"
    } else if last.ends_with("FAILED") {
        "FAILED"
    } else if last.contains("ROLLBACK") && kind != "ROLLBACK" && kind != "CONTINUE_ROLLBACK" {
        // An operation that rolled back did not do what it was asked to.
        "FAILED"
    } else {
        "SUCCEEDED"
    }
}

impl Core {
    /// Notes the operation an event belongs to, the first time one of its events is written.
    pub(crate) fn note_operation(&mut self, stack_id: &str, op_id: &str) {
        if self.operations.get(op_id).is_some() {
            return;
        }
        let Some(s) = self.stacks.get(stack_id) else {
            return;
        };
        let Some((_, last)) = s.last_op.as_ref().filter(|(id, _)| id == op_id) else {
            return;
        };
        let kind = kind_of(last).to_owned();
        let op = Operation {
            stack_id: stack_id.to_owned(),
            change_set: s.change_set_id.clone().filter(|_| kind.ends_with("_STACK")),
            kind,
        };
        self.operations.insert(op_id.to_owned(), op);
    }

    pub(crate) fn describe_events(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let events = self.chosen_events(r)?;
        let failed_only = r.v.at("Filters").at("FailedEvents").as_bool() == Some(true);
        let ops = self.operations_of(&events);
        let mut items: Vec<Value> = events
            .iter()
            .filter(|e| !failed_only || e.status.ends_with("FAILED"))
            .map(|e| operation_event(e, e.op_id.as_ref().and_then(|o| ops.get(o))))
            .collect();
        items.reverse();
        let start: usize = r.opt("NextToken").and_then(|t| t.parse().ok()).unwrap_or(0);
        let total = items.len();
        let taken: Vec<Value> = items.into_iter().skip(start).take(PAGE).collect();
        let mut v = json!({"OperationEvents": taken});
        if start + PAGE < total {
            v.put("NextToken", (start + PAGE).to_string().into());
        }
        Ok(v)
    }

    /// The events the request asks for, oldest first.
    fn chosen_events(&self, r: &Req<'_>) -> Result<Vec<&Event>, ApiError> {
        let of_stack = |id: &str| -> Vec<&Event> {
            // In key order: a stack's events sort together, oldest first.
            self.events
                .iter()
                .map(|(_, e)| e)
                .filter(|e| e.stack_id == id)
                .collect()
        };
        if let Some(cs) = r.opt("ChangeSetName") {
            let c = self.change_set(r, cs)?;
            let ids: Vec<String> = self
                .operations
                .iter()
                .filter(|(_, o)| o.change_set.as_deref() == Some(c.id.as_str()))
                .map(|(k, _)| k.to_owned())
                .collect();
            return Ok(of_stack(&c.stack_id)
                .into_iter()
                .filter(|e| e.op_id.as_ref().is_some_and(|o| ids.contains(o)))
                .collect());
        }
        if let Some(op) = r.opt("OperationId") {
            let stack = match r.opt("StackName") {
                Some(_) => Some(self.named(r)?.id.clone()),
                None => self.operations.get(op).map(|o| o.stack_id.clone()),
            };
            let pool: Vec<&Event> = match stack {
                Some(id) => of_stack(&id),
                None => self.events.iter().map(|(_, e)| e).collect(),
            };
            return Ok(pool
                .into_iter()
                .filter(|e| e.op_id.as_deref() == Some(op))
                .collect());
        }
        if r.opt("StackName").is_some() {
            return Ok(of_stack(&self.named(r)?.id.clone()));
        }
        // (memory) The model: "One of ChangeSetName, OperationId or StackName must be specified as input."
        Err(validation(
            "One of ChangeSetName, OperationId or StackName must be specified.",
        ))
    }

    /// What each operation among `events` is, and how it stands now.
    fn operations_of(&self, events: &[&Event]) -> BTreeMap<String, Op> {
        let mut out: BTreeMap<String, Op> = BTreeMap::new();
        for e in events {
            let Some(id) = &e.op_id else { continue };
            let op = out.entry(id.clone()).or_insert_with(|| Op {
                kind: self
                    .operations
                    .get(id)
                    .map_or_else(String::new, |o| o.kind.clone()),
                ..Op::default()
            });
            op.started.entry(e.logical.clone()).or_insert(e.at_ms);
        }
        for (id, op) in &mut out {
            // The operation's last stack-level event says how it stands.
            let last = events
                .iter()
                .rev()
                .filter(|e| {
                    e.op_id.as_deref() == Some(id.as_str()) && e.ty == "AWS::CloudFormation::Stack"
                })
                .find(|e| e.logical == e.stack_name)
                .map_or("", |e| e.status.as_str());
            op.status = op_status(&op.kind, last);
        }
        out
    }
}

/// One event as `OperationEvent`.
fn operation_event(e: &Event, op: Option<&Op>) -> Value {
    let mut o = Map::new();
    o.insert("EventId".into(), e.id.clone().into());
    o.insert("StackId".into(), e.stack_id.clone().into());
    if let Some(id) = &e.op_id {
        o.insert("OperationId".into(), id.clone().into());
    }
    if let Some(op) = op {
        if !op.kind.is_empty() {
            o.insert("OperationType".into(), op.kind.clone().into());
        }
        o.insert("OperationStatus".into(), op.status.into());
    }
    let is_stack = e.ty == "AWS::CloudFormation::Stack" && e.logical == e.stack_name;
    let kind = if is_stack {
        "STACK_EVENT"
    } else if e.status.ends_with("FAILED") {
        "PROVISIONING_ERROR"
    } else {
        "PROGRESS_EVENT"
    };
    o.insert("EventType".into(), kind.into());
    o.insert("LogicalResourceId".into(), e.logical.clone().into());
    o.insert("PhysicalResourceId".into(), e.physical.clone().into());
    o.insert("ResourceType".into(), e.ty.clone().into());
    o.insert("Timestamp".into(), secs(e.at_ms));
    let start = op
        .and_then(|op| op.started.get(&e.logical))
        .copied()
        .unwrap_or(e.at_ms);
    o.insert("StartTime".into(), secs(start));
    if !e.status.ends_with("_IN_PROGRESS") {
        o.insert("EndTime".into(), secs(e.at_ms));
    }
    o.insert("ResourceStatus".into(), e.status.clone().into());
    let mut put = |k: &str, v: &Option<String>| {
        if let Some(v) = v {
            o.insert(k.to_owned(), v.clone().into());
        }
    };
    put("ResourceStatusReason", &e.reason);
    put("ResourceProperties", &e.props);
    put("ClientRequestToken", &e.token);
    put("DetailedStatus", &e.detailed);
    Value::Object(o)
}
