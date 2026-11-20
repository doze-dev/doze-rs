//! Hooks and account limits: answers about what doze-aws does not run or enforce, given truthfully.
//!
//! doze-aws activates no hooks (the extension registry is not planned), so no hook is ever invoked: a change set has none
//! (`DescribeChangeSetHooks`), there are no hook results to list (`ListHookResults`) or read (`GetHookResult`). Each still checks
//! what it is asked about, so a wrong change set or target is refused as it would be.
//!
//! `DescribeAccountLimits` answers AWS's default quotas (memory, from the quotas page of the user guide: 2000 stacks, 200 outputs
//! per template, 2500 concurrent resources). doze-aws enforces none of them.

use serde_json::{Value, json};

use aws_wire::ApiError;

use crate::core::Core;
use crate::errors::validation;
use crate::req::Req;

/// AWS's default CloudFormation quotas, in the order `DescribeAccountLimits` gives them (memory).
const LIMITS: &[(&str, u32)] = &[
    ("StackLimit", 2000),
    ("StackOutputsLimit", 200),
    ("ConcurrentResourcesLimit", 2500),
];

/// `HookResultNotFoundException`: (memory) the words; the reason is doze-aws's.
fn no_results(what: &str) -> ApiError {
    ApiError::modeled(
        "HookResultNotFoundException",
        format!("No hook results were found for {what}: doze-aws runs no CloudFormation hooks"),
    )
}

impl Core {
    pub(crate) fn describe_change_set_hooks(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let c = self.change_set(r, r.s("ChangeSetName"))?;
        // Recorded 2026-11-19: with no hook activated, a created change set's hooks are PLANNED (and there are none).
        Ok(json!({
            "ChangeSetId": c.id,
            "ChangeSetName": c.name,
            "Hooks": [],
            "Status": "PLANNED",
            "StackId": c.stack_id,
            "StackName": c.stack_name,
        }))
    }

    pub(crate) fn list_hook_results(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (ty, id) = (r.opt("TargetType"), r.opt("TargetId"));
        if ty.is_some() != id.is_some() {
            // (memory) The model: TargetType and TargetId are given together.
            return Err(validation(
                "TargetType and TargetId must be specified together.",
            ));
        }
        if r.opt("Status").is_some() && r.opt("TypeArn").is_none() {
            // (memory) The model: Status can only be used in combination with TypeArn.
            return Err(validation("Status can only be specified with TypeArn."));
        }
        let (Some(ty), Some(id)) = (ty, id) else {
            return Ok(json!({"HookResults": []}));
        };
        let known = match ty {
            "CHANGE_SET" => self.change_sets.get(id).is_some(),
            "STACK" => self.find(id).is_some(),
            _ => false,
        };
        if !known {
            return Err(no_results(&format!("target {id}")));
        }
        Ok(json!({"TargetType": ty, "TargetId": id, "HookResults": []}))
    }

    pub(crate) fn get_hook_result(_r: &Req<'_>) -> Result<Value, ApiError> {
        // Recorded 2026-11-19.
        Err(ApiError::modeled(
            "HookResultNotFoundException",
            "The hook result was not found.",
        ))
    }

    pub(crate) fn describe_account_limits() -> Value {
        let limits: Vec<Value> = LIMITS
            .iter()
            .map(|(name, value)| json!({"Name": name, "Value": value}))
            .collect();
        json!({"AccountLimits": limits})
    }
}
