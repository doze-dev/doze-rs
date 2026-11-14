//! CreateStack, UpdateStack, DeleteStack, RollbackStack and ContinueUpdateRollback: each checks what AWS checks at the call, records
//! the stack in progress, and starts the job that does the work.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use aws_wire::ApiError;
use aws_wire::service::Caller;

use crate::core::Core;
use crate::engine::{Job, Kind, OnFailure};
use crate::errors::{cannot_update, exists, no_stack_bracketed, validation};
use crate::model::{Draft, Previous, Stack};
use crate::prepare::Caps;
use crate::req::Req;
use crate::tmpl::eval::Pseudo;

/// A new stack's id.
pub(crate) fn stack_arn(region: &str, account: &str, name: &str) -> String {
    format!(
        "arn:{}:cloudformation:{region}:{account}:stack/{name}/{}",
        crate::tmpl::eval::partition(region),
        aws_seal::uuid_v4().unwrap_or_default()
    )
}

/// What a failed create does, from `OnFailure` and `DisableRollback`.
pub(crate) fn on_failure(r: &Req<'_>) -> Result<OnFailure, ApiError> {
    match (r.opt("OnFailure"), r.flag("DisableRollback")) {
        // (memory)
        (Some(_), Some(_)) => Err(validation(
            "You cannot specify both DisableRollback and OnFailure.",
        )),
        (Some("DELETE"), None) => Ok(OnFailure::Delete),
        (Some("DO_NOTHING"), None) | (None, Some(true)) => Ok(OnFailure::DoNothing),
        _ => Ok(OnFailure::Rollback),
    }
}

impl Core {
    /// The stack a name or id names: by id any stack (a deleted one too), by name a live one.
    pub(crate) fn find(&self, name_or_id: &str) -> Option<&Stack> {
        if name_or_id.starts_with("arn:") {
            return self.stacks.get(name_or_id);
        }
        self.stacks
            .iter()
            .map(|(_, s)| s)
            .find(|s| s.name == name_or_id && s.status != "DELETE_COMPLETE")
    }

    /// Every export of the region: name to value and exporting stack.
    pub(crate) fn exports(&self) -> BTreeMap<String, (String, String)> {
        let mut out = BTreeMap::new();
        for (_, s) in self
            .stacks
            .iter()
            .filter(|(_, s)| s.status != "DELETE_COMPLETE" && s.status != "REVIEW_IN_PROGRESS")
        {
            for o in &s.outputs {
                if let Some(e) = &o.export {
                    out.insert(e.clone(), (o.value.clone(), s.id.clone()));
                }
            }
        }
        out
    }

    pub(crate) fn pseudo(caller: &Caller, id: &str, name: &str, arns: &[String]) -> Pseudo {
        Pseudo {
            account: caller.account_id.clone(),
            region: caller.region.clone(),
            stack_id: id.to_owned(),
            stack_name: name.to_owned(),
            notification_arns: arns.to_vec(),
        }
    }

    /// A job for the stack `id` as it is recorded now.
    pub(crate) fn job_for(
        &self,
        kind: Kind,
        id: &str,
        caller: &Caller,
        on_failure: OnFailure,
    ) -> Option<Job> {
        let s = self.stacks.get(id)?;
        Some(Job {
            kind,
            stack_id: s.id.clone(),
            stack_name: s.name.clone(),
            caller: caller.clone(),
            template: serde_json::from_str(&s.processed).unwrap_or(Value::Null),
            params: s.params.clone(),
            previous: s.previous.clone(),
            current: s.resources.clone(),
            on_failure,
            retain: BTreeSet::new(),
            retain_except_on_create: s.retain_except_on_create,
            exports: self.exports(),
            tags: s.tags.clone(),
            notification_arns: s.notification_arns.clone(),
            change_set: None,
            policy: s.policy.clone(),
            done: None,
            tags_changed: false,
        })
    }

    pub(crate) async fn create_stack(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("StackName").to_owned();
        check_name(&name)?;
        if self.find(&name).is_some() {
            return Err(exists(&name));
        }
        let on_failure = on_failure(r)?;
        let id = stack_arn(r.region(), r.account(), &name);
        let arns = r.strings("NotificationARNs");
        let pseudo = Self::pseudo(&r.call.caller, &id, &name, &arns);
        let body = self.template_text(r, None).await?;
        let p = self.prepare(r, body, &[], &pseudo, Caps::Stack).await?;
        let policy = self
            .policy_text(r, "StackPolicyBody", "StackPolicyURL")
            .await?;
        let op = aws_seal::uuid_v4().unwrap_or_default();
        let stack = Stack {
            id: id.clone(),
            name: name.clone(),
            status: "CREATE_IN_PROGRESS".into(),
            created_ms: self.now_ms(),
            description: p.description.clone(),
            body: p.body.clone(),
            processed: p.processed.to_string(),
            params: p.params.clone(),
            tags: r.tags().unwrap_or_default(),
            capabilities: r.strings("Capabilities"),
            notification_arns: arns,
            disable_rollback: on_failure == OnFailure::DoNothing,
            termination_protection: r.flag("EnableTerminationProtection") == Some(true),
            role_arn: r.opt("RoleARN").map(str::to_owned),
            timeout: r.int("TimeoutInMinutes"),
            policy,
            last_op: Some((op.clone(), "CREATE_STACK".into())),
            retain_except_on_create: r.flag("RetainExceptOnCreate") == Some(true),
            ..Stack::default()
        };
        self.stacks.insert(id.clone(), stack);
        self.push_event(
            &id,
            Draft::stack(&name, &id, "CREATE_IN_PROGRESS", Some("User Initiated")),
        );
        if let Some(job) = self.job_for(Kind::Create, &id, &r.call.caller, on_failure) {
            self.started.push(job);
        }
        Ok(json!({"StackId": id, "OperationId": op}))
    }

    /// A stack policy from a body or a URL.
    pub(crate) async fn policy_text(
        &self,
        r: &Req<'_>,
        body: &str,
        url: &str,
    ) -> Result<Option<String>, ApiError> {
        match (r.opt(body), r.opt(url)) {
            (Some(b), _) => Ok(Some(b.to_owned())),
            (None, Some(u)) => {
                crate::transform::include::fetch(self.peers.as_deref(), &r.call.caller, u)
                    .await
                    .map(Some)
            }
            (None, None) => Ok(None),
        }
    }

    /// Why a stack in `s`'s status cannot be updated (recorded 2026-11-12 for `ROLLBACK_COMPLETE` and `CREATE_FAILED`).
    fn updatable(s: &Stack, disable_rollback: bool) -> Result<(), ApiError> {
        match s.status.as_str() {
            "CREATE_COMPLETE"
            | "UPDATE_COMPLETE"
            | "UPDATE_ROLLBACK_COMPLETE"
            | "IMPORT_COMPLETE"
            | "IMPORT_ROLLBACK_COMPLETE" => Ok(()),
            "CREATE_FAILED" | "UPDATE_FAILED" if disable_rollback => Ok(()),
            st @ ("CREATE_FAILED" | "UPDATE_FAILED") => Err(validation(format!(
                "This stack is currently in a non-terminal [{st}] state. To update the stack from this state, please use the disable-rollback parameter with update-stack API. To rollback to the last known good state, use the rollback-stack API"
            ))),
            st => Err(cannot_update(&s.id, st)),
        }
    }

    /// The stack an update names, if it may be updated, and the notification topics it will have.
    fn update_target(&self, r: &Req<'_>) -> Result<(Stack, Vec<String>), ApiError> {
        let name = r.s("StackName");
        let Some(stack) = self
            .find(name)
            .filter(|s| s.status != "DELETE_COMPLETE")
            .cloned()
        else {
            return Err(no_stack_bracketed(name));
        };
        Self::updatable(&stack, r.flag("DisableRollback") == Some(true))?;
        let arns = if r.v.get("NotificationARNs").is_some() {
            r.strings("NotificationARNs")
        } else {
            stack.notification_arns.clone()
        };
        Ok((stack, arns))
    }

    pub(crate) async fn update_stack(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (stack, arns) = self.update_target(r)?;
        let pseudo = Self::pseudo(&r.call.caller, &stack.id, &stack.name, &arns);
        let body = self.template_text(r, Some(&stack)).await?;
        let p = self
            .prepare(r, body, &stack.params, &pseudo, Caps::Stack)
            .await?;
        let old: Value = serde_json::from_str(&stack.processed).unwrap_or(Value::Null);
        check_types(&old, &p.processed)?;
        let tags = r.tags().unwrap_or_else(|| stack.tags.clone());
        if p.processed == old
            && p.params == stack.params
            && tags == stack.tags
            && arns == stack.notification_arns
        {
            // Recorded 2026-11-12.
            return Err(validation("No updates are to be performed."));
        }
        let policy = self
            .policy_text(r, "StackPolicyBody", "StackPolicyURL")
            .await?;
        let op = aws_seal::uuid_v4().unwrap_or_default();
        let now = self.now_ms();
        let disable = r.flag("DisableRollback") == Some(true);
        let tags_changed = tags != stack.tags;
        if let Some(s) = self.stacks.get_mut(&stack.id) {
            begin_update(s, &p, now);
            s.tags.clone_from(&tags);
            s.notification_arns = arns;
            s.capabilities = r.strings("Capabilities");
            s.disable_rollback = disable;
            s.last_op = Some((op.clone(), "UPDATE_STACK".into()));
            if policy.is_some() {
                s.policy = policy;
            }
        }
        self.push_event(
            &stack.id,
            Draft::stack(
                &stack.name,
                &stack.id,
                "UPDATE_IN_PROGRESS",
                Some("User Initiated"),
            ),
        );
        let failure = if disable {
            OnFailure::DoNothing
        } else {
            OnFailure::Rollback
        };
        if let Some(mut job) = self.job_for(Kind::Update, &stack.id, &r.call.caller, failure) {
            job.tags_changed = tags_changed;
            if let Some(during) = self
                .policy_text(
                    r,
                    "StackPolicyDuringUpdateBody",
                    "StackPolicyDuringUpdateURL",
                )
                .await?
            {
                job.policy = Some(during);
            }
            self.started.push(job);
        }
        Ok(json!({"StackId": stack.id, "OperationId": op}))
    }

    pub(crate) fn delete_stack(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("StackName");
        let Some(stack) = self.find(name).cloned() else {
            // Recorded 2026-11-12: deleting a stack that is not there is not an error.
            return Ok(json!({}));
        };
        if stack.status == "DELETE_COMPLETE" || stack.status == "DELETE_IN_PROGRESS" {
            return Ok(json!({}));
        }
        let retain = deletable(&stack, r)?;
        let op = aws_seal::uuid_v4().unwrap_or_default();
        if let Some(s) = self.stacks.get_mut(&stack.id) {
            s.last_op = Some((op, "DELETE_STACK".into()));
        }
        if stack.status == "REVIEW_IN_PROGRESS" {
            return Ok(self.forget_review(&stack));
        }
        if let Some(user) = self.importer_of(&stack) {
            let in_progress = Draft::stack(
                &stack.name,
                &stack.id,
                "DELETE_IN_PROGRESS",
                Some("User Initiated"),
            );
            self.push_event(&stack.id, in_progress);
            self.push_event(
                &stack.id,
                Draft::stack(&stack.name, &stack.id, "DELETE_FAILED", Some(&user)),
            );
            if let Some(s) = self.stacks.get_mut(&stack.id) {
                s.status = "DELETE_FAILED".into();
                s.reason = Some(user);
            }
            return Ok(json!({}));
        }
        if let Some(s) = self.stacks.get_mut(&stack.id) {
            s.status = "DELETE_IN_PROGRESS".into();
            s.reason = None;
        }
        let in_progress = Draft::stack(
            &stack.name,
            &stack.id,
            "DELETE_IN_PROGRESS",
            Some("User Initiated"),
        );
        self.push_event(&stack.id, in_progress);
        if let Some(mut job) = self.job_for(
            Kind::Delete,
            &stack.id,
            &r.call.caller,
            OnFailure::DoNothing,
        ) {
            job.retain = retain;
            self.started.push(job);
        }
        Ok(json!({}))
    }
}

/// An update begins: the template and parameters are the new ones, and the old ones are kept for a rollback.
fn begin_update(s: &mut Stack, p: &crate::prepare::Prepared, now: i64) {
    s.previous = Some(Previous {
        body: s.body.clone(),
        processed: s.processed.clone(),
        params: s.params.clone(),
        description: s.description.clone(),
        resources: s.resources.clone(),
    });
    s.status = "UPDATE_IN_PROGRESS".into();
    s.reason = None;
    s.updated_ms = Some(now);
    s.body.clone_from(&p.body);
    s.processed = p.processed.to_string();
    s.params.clone_from(&p.params);
    s.description.clone_from(&p.description);
    s.change_set_id = None;
}

/// Whether a stack may be deleted now, and the resources the delete leaves in place.
fn deletable(stack: &Stack, r: &Req<'_>) -> Result<BTreeSet<String>, ApiError> {
    {
        if stack.termination_protection {
            // (memory)
            return Err(validation(format!(
                "Stack [{}] cannot be deleted while TerminationProtection is enabled",
                stack.name
            )));
        }
        if stack.parent_id.is_some() && r.call.caller.service.as_deref() != Some("cloudformation") {
            // (memory) A nested stack goes with its root.
            return Err(validation(format!(
                "Stack [{}] is a nested stack: delete its root stack instead",
                stack.name
            )));
        }
        if stack.status.ends_with("_IN_PROGRESS") && stack.status != "REVIEW_IN_PROGRESS" {
            // (memory)
            return Err(validation(format!(
                "Stack [{}] is in {} state and can not be deleted",
                stack.name, stack.status
            )));
        }
        let retain: BTreeSet<String> = r.strings("RetainResources").into_iter().collect();
        if !retain.is_empty() && stack.status != "DELETE_FAILED" {
            // (memory)
            return Err(validation(
                "Invalid operation on stack. RetainResources may only be specified when the stack is in the DELETE_FAILED state",
            ));
        }
        Ok(retain)
    }
}

/// A new stack's name: a letter, then letters, digits and hyphens, at most 128. (memory) The recorder's policy refused the recorded
/// case before CloudFormation saw it, so the words are the Coral front end's.
pub(crate) fn check_name(name: &str) -> Result<(), ApiError> {
    let ok = name.len() <= 128
        && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if ok {
        return Ok(());
    }
    Err(validation(format!(
        "1 validation error detected: Value '{name}' at 'stackName' failed to satisfy constraint: Member must satisfy regular expression pattern: [a-zA-Z][-a-zA-Z0-9]*|arn:[-a-zA-Z0-9:/._+]*"
    )))
}

/// A resource may not change type in an update.
fn check_types(old: &Value, new: &Value) -> Result<(), ApiError> {
    let (Some(a), Some(b)) = (crate::tmpl::resources(old), crate::tmpl::resources(new)) else {
        return Ok(());
    };
    let changed: Vec<&String> = b
        .iter()
        .filter(|(k, v)| {
            a.get(*k)
                .is_some_and(|o| crate::tmpl::type_of(o) != crate::tmpl::type_of(v))
        })
        .map(|(k, _)| k)
        .collect();
    if changed.is_empty() {
        return Ok(());
    }
    // (memory)
    Err(validation(format!(
        "Update of resource type is not permitted. The new template modifies resource type of the following resources: [{}]",
        changed
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    )))
}
