//! Change sets: made with their change records (a `CREATE` change set brings its stack into being in `REVIEW_IN_PROGRESS`), described,
//! listed, deleted, and executed (which is a create or an update of the stack from what the change set holds).

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use aws_wire::ApiError;

use crate::core::Core;
use crate::describe::secs;
use crate::engine::{Kind, OnFailure, Working};
use crate::errors::{no_change_set, no_stack_bracketed, validation};
use crate::j::J;
use crate::model::{ChangeSet, Draft, Previous, Stack};
use crate::prepare::Caps;
use crate::req::Req;
use crate::stacks::stack_arn;

/// The words AWS gives a change set with nothing in it (the AWS CLI looks for them).
const NO_CHANGES: &str = "The submitted information didn't contain changes. Submit different information to create a change set.";

impl Core {
    /// The change set a request names (by ARN, or by name within the stack it names).
    pub(crate) fn change_set(&self, r: &Req<'_>, name: &str) -> Result<&ChangeSet, ApiError> {
        if name.starts_with("arn:") {
            return self
                .change_sets
                .get(name)
                .ok_or_else(|| no_change_set(name));
        }
        let stack = r
            .opt("StackName")
            .and_then(|s| self.find(s))
            .map(|s| s.id.clone());
        let Some(stack) = stack else {
            if r.opt("StackName").is_none() {
                // (memory)
                return Err(validation(
                    "StackName must be specified if ChangeSetName is not specified as an ARN.",
                ));
            }
            return Err(no_change_set(name));
        };
        self.change_sets
            .iter()
            .map(|(_, c)| c)
            .find(|c| c.stack_id == stack && c.name == name)
            .ok_or_else(|| no_change_set(name))
    }

    /// The stack a change set is for: an existing one for `UPDATE`, a new one in review for `CREATE`.
    fn change_set_stack(&self, r: &Req<'_>, kind: &str) -> Result<Stack, ApiError> {
        let stack_name = r.s("StackName").to_owned();
        let cs_name = r.s("ChangeSetName");
        let existing = self.find(&stack_name).cloned();
        if kind == "CREATE" && !stack_name.starts_with("arn:") {
            crate::stacks::check_name(&stack_name)?;
        }
        let stack = match (kind, existing) {
            ("CREATE", Some(s)) if s.status == "REVIEW_IN_PROGRESS" => s,
            // (memory)
            ("CREATE", Some(_)) => {
                return Err(validation(format!(
                    "Stack [{stack_name}] already exists and cannot be created again with the changeSet [{cs_name}]."
                )));
            }
            ("CREATE", None) => Stack {
                id: stack_arn(r.region(), r.account(), &stack_name),
                name: stack_name.clone(),
                status: "REVIEW_IN_PROGRESS".into(),
                created_ms: self.now_ms(),
                ..Stack::default()
            },
            (_, Some(s)) if s.status != "REVIEW_IN_PROGRESS" && s.status != "DELETE_COMPLETE" => s,
            (_, Some(s)) if s.status == "REVIEW_IN_PROGRESS" => {
                return Err(crate::errors::cannot_update(&s.id, &s.status));
            }
            _ => return Err(no_stack_bracketed(&stack_name)),
        };
        if self
            .change_sets
            .iter()
            .any(|(_, c)| c.stack_id == stack.id && c.name == cs_name)
        {
            // (memory)
            return Err(ApiError::modeled(
                "AlreadyExistsException",
                format!("ChangeSet [{cs_name}] already exists"),
            ));
        }
        Ok(stack)
    }

    /// The change records for `p` against `stack`, and the parameters whose values changed.
    fn diff(
        &self,
        stack: &Stack,
        p: &crate::prepare::Prepared,
        pseudo: crate::tmpl::eval::Pseudo,
        old: &Value,
    ) -> Result<(Vec<Value>, BTreeSet<String>), ApiError> {
        let changed: BTreeSet<String> = p
            .params
            .iter()
            .filter(|b| {
                !stack
                    .params
                    .iter()
                    .any(|o| o.key == b.key && o.value == b.value && o.resolved == b.resolved)
            })
            .map(|b| b.key.clone())
            .collect();
        let exports = self
            .exports()
            .into_iter()
            .map(|(k, (v, _))| (k, v))
            .collect();
        let w = Working::new(
            p.processed.clone(),
            &p.params,
            pseudo,
            exports,
            stack.resources.clone(),
        )
        .map_err(validation)?;
        Ok((records(&w, stack, old, &changed), changed))
    }

    pub(crate) async fn create_change_set(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let kind = r.opt("ChangeSetType").unwrap_or("UPDATE").to_owned();
        if kind == "IMPORT" {
            return Err(ApiError::new(
                501,
                "NotImplemented",
                "Resource import is not built in doze-aws.",
            )
            .with("X-Doze-Not-Built", "1"));
        }
        let cs_name = r.s("ChangeSetName").to_owned();
        let stack = self.change_set_stack(r, &kind)?;
        let arns = if r.v.get("NotificationARNs").is_some() {
            r.strings("NotificationARNs")
        } else {
            stack.notification_arns.clone()
        };
        let pseudo = Self::pseudo(&r.call.caller, &stack.id, &stack.name, &arns);
        let body = self
            .template_text(r, (kind == "UPDATE").then_some(&stack))
            .await?;
        let p = self
            .prepare(r, body, &stack.params, &pseudo, Caps::ChangeSet)
            .await?;
        let old: Value = serde_json::from_str(&stack.processed).unwrap_or(Value::Null);
        let (changes, changed) = self.diff(&stack, &p, pseudo, &old)?;
        let tags = r.tags().unwrap_or_else(|| stack.tags.clone());
        let nothing =
            changes.is_empty() && p.processed == old && changed.is_empty() && tags == stack.tags;
        let cs = ChangeSet {
            name: cs_name,
            stack_id: stack.id.clone(),
            stack_name: stack.name.clone(),
            created_ms: self.now_ms(),
            status: if nothing { "FAILED" } else { "CREATE_COMPLETE" }.into(),
            reason: nothing.then(|| NO_CHANGES.to_owned()),
            execution: if nothing { "UNAVAILABLE" } else { "AVAILABLE" }.into(),
            kind: kind.clone(),
            body: p.body,
            processed: p.processed.to_string(),
            params: p.params,
            tags,
            notification_arns: arns,
            changes: Value::Array(changes).to_string(),
            ..from_request(r)
        };
        let id = cs.id.clone();
        if kind == "CREATE" && self.stacks.get(&stack.id).is_none() {
            let (sid, sname) = (stack.id.clone(), stack.name.clone());
            self.stacks.insert(stack.id.clone(), stack);
            self.push_event(
                &sid,
                Draft::stack(&sname, &sid, "REVIEW_IN_PROGRESS", Some("User Initiated")),
            );
        }
        let stack_id = cs.stack_id.clone();
        self.change_sets.insert(id.clone(), cs);
        Ok(json!({"Id": id, "StackId": stack_id}))
    }

    pub(crate) fn describe_change_set(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let c = self.change_set(r, r.s("ChangeSetName"))?;
        let mut o = Map::new();
        o.insert("ChangeSetName".into(), c.name.clone().into());
        o.insert("ChangeSetId".into(), c.id.clone().into());
        o.insert("StackId".into(), c.stack_id.clone().into());
        o.insert("StackName".into(), c.stack_name.clone().into());
        if let Some(d) = &c.description {
            o.insert("Description".into(), d.clone().into());
        }
        if !c.params.is_empty() {
            o.insert(
                "Parameters".into(),
                Value::Array(crate::tmpl::params::described(&c.params)),
            );
        }
        o.insert("CreationTime".into(), secs(c.created_ms));
        o.insert("ExecutionStatus".into(), c.execution.clone().into());
        o.insert("Status".into(), c.status.clone().into());
        if let Some(rsn) = &c.reason {
            o.insert("StatusReason".into(), rsn.clone().into());
        }
        o.insert("NotificationARNs".into(), json!(c.notification_arns));
        o.insert("RollbackConfiguration".into(), json!({}));
        o.insert("Capabilities".into(), json!(c.capabilities));
        if !c.tags.is_empty() {
            o.insert(
                "Tags".into(),
                Value::Array(
                    c.tags
                        .iter()
                        .map(|(k, v)| json!({"Key": k, "Value": v}))
                        .collect(),
                ),
            );
        }
        o.insert(
            "Changes".into(),
            serde_json::from_str(&c.changes).unwrap_or(json!([])),
        );
        o.insert("IncludeNestedStacks".into(), c.include_nested.into());
        if let Some(f) = &c.on_failure {
            o.insert("OnStackFailure".into(), f.clone().into());
        }
        o.insert(
            "DeploymentConfig".into(),
            json!({"DisableRollback": c.disable_rollback, "Mode": "STANDARD"}),
        );
        Ok(Value::Object(o))
    }

    pub(crate) fn list_change_sets(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let s = self.named(r)?;
        let items: Vec<Value> = self
            .change_sets
            .iter()
            .map(|(_, c)| c)
            .filter(|c| c.stack_id == s.id)
            .map(|c| {
                let mut v = json!({"StackId": c.stack_id, "StackName": c.stack_name, "ChangeSetId": c.id, "ChangeSetName": c.name,
                    "ExecutionStatus": c.execution, "Status": c.status, "CreationTime": secs(c.created_ms),
                    "IncludeNestedStacks": c.include_nested});
                if let Some(d) = &c.description {
                    v.put("Description", d.clone().into());
                }
                if let Some(rsn) = &c.reason {
                    v.put("StatusReason", rsn.clone().into());
                }
                v
            })
            .collect();
        Ok(json!({"Summaries": items}))
    }

    pub(crate) fn delete_change_set(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let c = self.change_set(r, r.s("ChangeSetName"))?.clone();
        if c.execution == "EXECUTE_IN_PROGRESS" {
            // (memory)
            return Err(ApiError::modeled(
                "InvalidChangeSetStatusException",
                format!(
                    "Cannot delete ChangeSet [{}] in status EXECUTE_IN_PROGRESS",
                    c.id
                ),
            ));
        }
        self.change_sets.remove(&c.id);
        Ok(json!({}))
    }

    pub(crate) fn execute_change_set(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let c = self.change_set(r, r.s("ChangeSetName"))?.clone();
        if c.execution != "AVAILABLE" {
            // Recorded 2026-11-12.
            return Err(ApiError::modeled(
                "InvalidChangeSetStatusException",
                format!(
                    "ChangeSet [{}] cannot be executed in its current execution status of [{}]",
                    c.id, c.execution
                ),
            ));
        }
        let Some(stack) = self.stacks.get(&c.stack_id).cloned() else {
            return Err(no_change_set(&c.name));
        };
        let creating = stack.status == "REVIEW_IN_PROGRESS";
        let disable = r.flag("DisableRollback") == Some(true)
            || c.on_failure.as_deref() == Some("DO_NOTHING");
        let on_failure = match (disable, c.on_failure.as_deref()) {
            (true, _) => OnFailure::DoNothing,
            (false, Some("DELETE")) if creating => OnFailure::Delete,
            _ => OnFailure::Rollback,
        };
        let op = aws_seal::uuid_v4().unwrap_or_default();
        let now = self.now_ms();
        let status = if creating {
            "CREATE_IN_PROGRESS"
        } else {
            "UPDATE_IN_PROGRESS"
        };
        let tags_changed = c.tags != stack.tags;
        if let Some(s) = self.stacks.get_mut(&stack.id) {
            take_change_set(s, &c, creating, now);
            s.status = status.into();
            s.disable_rollback = disable;
            s.last_op = Some((
                op,
                if creating {
                    "CREATE_STACK"
                } else {
                    "UPDATE_STACK"
                }
                .into(),
            ));
        }
        for (_, other) in self
            .change_sets
            .iter()
            .filter(|(_, x)| x.stack_id == stack.id && x.id != c.id && x.execution == "AVAILABLE")
            .map(|(k, x)| (k.to_owned(), x.id.clone()))
            .collect::<Vec<_>>()
        {
            if let Some(x) = self.change_sets.get_mut(&other) {
                x.execution = "OBSOLETE".into();
            }
        }
        if let Some(x) = self.change_sets.get_mut(&c.id) {
            x.execution = "EXECUTE_IN_PROGRESS".into();
        }
        self.push_event(
            &stack.id,
            Draft::stack(&stack.name, &stack.id, status, Some("User Initiated")),
        );
        let kind = if creating { Kind::Create } else { Kind::Update };
        if let Some(mut job) = self.job_for(kind, &stack.id, &r.call.caller, on_failure) {
            job.change_set = Some(c.id);
            job.tags_changed = tags_changed;
            self.started.push(job);
        }
        Ok(json!({}))
    }
}

/// A stack takes what an executed change set holds (an update keeps what it was, for a rollback).
fn take_change_set(s: &mut Stack, c: &ChangeSet, creating: bool, now: i64) {
    if !creating {
        s.previous = Some(Previous {
            body: s.body.clone(),
            processed: s.processed.clone(),
            params: s.params.clone(),
            description: s.description.clone(),
            resources: s.resources.clone(),
        });
        s.updated_ms = Some(now);
    }
    s.reason = None;
    s.body.clone_from(&c.body);
    s.processed.clone_from(&c.processed);
    s.params.clone_from(&c.params);
    s.description = serde_json::from_str::<Value>(&c.processed)
        .ok()
        .and_then(|t| {
            t.get("Description")
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    s.tags.clone_from(&c.tags);
    s.capabilities.clone_from(&c.capabilities);
    s.notification_arns.clone_from(&c.notification_arns);
    s.role_arn.clone_from(&c.role_arn);
    s.change_set_id = Some(c.id.clone());
}

/// What a change set takes from the request as it stands.
fn from_request(r: &Req<'_>) -> ChangeSet {
    ChangeSet {
        id: format!(
            "arn:{}:cloudformation:{}:{}:changeSet/{}/{}",
            crate::tmpl::eval::partition(r.region()),
            r.region(),
            r.account(),
            r.s("ChangeSetName"),
            aws_seal::uuid_v4().unwrap_or_default()
        ),
        description: r.opt("Description").map(str::to_owned),
        capabilities: r.strings("Capabilities"),
        include_nested: r.flag("IncludeNestedStacks") == Some(true),
        on_failure: r.opt("OnStackFailure").map(str::to_owned),
        role_arn: r.opt("RoleARN").map(str::to_owned),
        ..ChangeSet::default()
    }
}

/// The change records: computed once, then again with what is added or replaced unknown, so what depends on it is `Dynamic`.
fn records(w: &Working, stack: &Stack, old: &Value, changed: &BTreeSet<String>) -> Vec<Value> {
    let first = crate::changes::compute(w, &stack.resources, old, changed);
    let moving: Vec<String> = first
        .iter()
        .filter(|c| {
            let rc = &c.at("ResourceChange");
            rc.at("Action") == "Add" || rc.at("Replacement") == "True"
        })
        .filter_map(|c| {
            c.at("ResourceChange")
                .at("LogicalResourceId")
                .as_str()
                .map(str::to_owned)
        })
        .collect();
    if moving.is_empty() {
        return first;
    }
    let mut w2 = w.clone();
    for m in &moving {
        w2.resources.remove(m);
    }
    let mut second = crate::changes::compute(&w2, &stack.resources, old, changed);
    // The second pass sees what moves as absent; it keeps their records from the first.
    for c in &mut second {
        let id = c
            .at("ResourceChange")
            .at("LogicalResourceId")
            .as_str()
            .unwrap_or_default()
            .to_owned();
        if moving.contains(&id)
            && let Some(f) = first
                .iter()
                .find(|f| f.at("ResourceChange").at("LogicalResourceId") == id.as_str())
        {
            *c = f.clone();
        }
    }
    second
}
