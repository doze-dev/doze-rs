//! RollbackStack and ContinueUpdateRollback, and what DeleteStack needs to know first (a stack still in review goes at once; a stack
//! whose export another imports cannot go).

use std::collections::BTreeSet;

use serde_json::{Value, json};

use aws_wire::ApiError;

use crate::core::Core;
use crate::engine::{Kind, OnFailure};
use crate::errors::validation;
use crate::model::{Draft, Stack};
use crate::req::Req;

impl Core {
    /// A stack that only ever had an unexecuted change set goes at once, with its change sets.
    pub(crate) fn forget_review(&mut self, stack: &Stack) -> Value {
        let now = self.now_ms();
        if let Some(s) = self.stacks.get_mut(&stack.id) {
            s.status = "DELETE_COMPLETE".into();
            s.deleted_ms = Some(now);
        }
        self.push_event(
            &stack.id,
            Draft::stack(&stack.name, &stack.id, "DELETE_COMPLETE", None),
        );
        let sets: Vec<String> = self
            .change_sets
            .iter()
            .filter(|(_, c)| c.stack_id == stack.id)
            .map(|(k, _)| k.to_owned())
            .collect();
        for k in sets {
            self.change_sets.remove(&k);
        }
        json!({})
    }

    /// Why a stack cannot be deleted while another imports one of its exports. (memory) for the words.
    pub(crate) fn importer_of(&self, stack: &Stack) -> Option<String> {
        for o in &stack.outputs {
            let Some(e) = &o.export else { continue };
            if let Some((_, user)) = self.stacks.iter().find(|(_, s)| {
                s.id != stack.id && s.status != "DELETE_COMPLETE" && s.imports.contains(e)
            }) {
                return Some(format!(
                    "Export {e} cannot be deleted as it is in use by {}",
                    user.name
                ));
            }
        }
        None
    }

    pub(crate) fn rollback_stack(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("StackName");
        let Some(stack) = self.find(name).cloned() else {
            return Err(crate::errors::no_stack(name));
        };
        let (kind, status) = match stack.status.as_str() {
            "CREATE_FAILED" => (Kind::RollbackCreate, "ROLLBACK_IN_PROGRESS"),
            "UPDATE_FAILED" if stack.previous.is_some() => {
                (Kind::RollbackUpdate, "UPDATE_ROLLBACK_IN_PROGRESS")
            }
            // (memory)
            st => {
                return Err(validation(format!(
                    "Stack {} is in {st} state and can not be rolled back",
                    stack.id
                )));
            }
        };
        Ok(self.start_rollback(r, &stack, kind, (status, "ROLLBACK_STACK"), BTreeSet::new()))
    }

    pub(crate) fn continue_update_rollback(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("StackName");
        let Some(stack) = self.find(name).cloned() else {
            return Err(crate::errors::no_stack(name));
        };
        if stack.status != "UPDATE_ROLLBACK_FAILED" || stack.previous.is_none() {
            // (memory)
            return Err(validation(format!(
                "Stack {} is in {} state and can not continue update rollback",
                stack.id, stack.status
            )));
        }
        let skip = r.strings("ResourcesToSkip").into_iter().collect();
        self.start_rollback(
            r,
            &stack,
            Kind::RollbackUpdate,
            ("UPDATE_ROLLBACK_IN_PROGRESS", "CONTINUE_ROLLBACK"),
            skip,
        );
        Ok(json!({}))
    }

    fn start_rollback(
        &mut self,
        r: &Req<'_>,
        stack: &Stack,
        kind: Kind,
        (status, op_type): (&str, &str),
        skip: BTreeSet<String>,
    ) -> Value {
        let op = aws_seal::uuid_v4().unwrap_or_default();
        if let Some(s) = self.stacks.get_mut(&stack.id) {
            s.status = status.to_owned();
            s.reason = None;
            s.last_op = Some((op.clone(), op_type.to_owned()));
        }
        self.push_event(
            &stack.id,
            Draft::stack(&stack.name, &stack.id, status, Some("User Initiated")),
        );
        if let Some(mut job) = self.job_for(kind, &stack.id, &r.call.caller, OnFailure::Rollback) {
            if kind == Kind::RollbackUpdate
                && let Some(p) = &stack.previous
            {
                job.template = serde_json::from_str(&p.processed).unwrap_or(Value::Null);
                job.params.clone_from(&p.params);
            }
            job.retain = skip;
            self.started.push(job);
        }
        json!({"StackId": stack.id, "OperationId": op})
    }
}
