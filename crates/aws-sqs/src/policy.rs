//! Queue policies, and `AddPermission` and `RemovePermission`, which edit one.
//!
//! The messages are those AWS gave in `test_invalid_policy`, `test_add_permission_rules` and
//! `test_add_permission_statement_limit` (recorded 2026-11-09). Two things differ from AWS on purpose and are recorded as
//! deviations: AWS checks that a permission's principal is an account that exists (it answered "Unable to verify" even for a
//! well-formed account id nobody holds), and a laptop has no list of accounts to check against, so any 12-digit id passes. The
//! message AWS gives at the statement limit is unrecorded.

use aws_model_sqs::keys::{AddPermissionRequest as Add, RemovePermissionRequest as Remove};
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};
use serde_json::{Value, json};

use crate::core::{Core, empty, invalid_parameter, missing, request};
use crate::model::Queue;

/// The most statements a queue's policy may hold: AWS refused the 21st.
const MAX_STATEMENTS: usize = 20;
/// The most actions one `AddPermission` may name.
const MAX_ACTIONS: usize = 7;

/// Actions another account may be granted.
const SHAREABLE: &[&str] = &[
    "SendMessage",
    "ReceiveMessage",
    "DeleteMessage",
    "ChangeMessageVisibility",
    "GetQueueAttributes",
    "GetQueueUrl",
    "ListDeadLetterSourceQueues",
    "ListQueueTags",
    "PurgeQueue",
    "*",
];
/// Actions only the queue's owner may take.
const OWNER_ONLY: &[&str] = &[
    "SetQueueAttributes",
    "AddPermission",
    "RemovePermission",
    "CreateQueue",
    "DeleteQueue",
    "ListQueues",
    "TagQueue",
    "UntagQueue",
    "StartMessageMoveTask",
    "CancelMessageMoveTask",
    "ListMessageMoveTasks",
];

fn invalid_policy(reason: Option<&str>) -> ApiError {
    let tail = reason.map_or_else(String::new, |r| format!(" Reason: {r}"));
    ApiError::new(
        400,
        "InvalidAttributeValue",
        format!("Invalid value for the parameter Policy.{tail}"),
    )
}

/// Checks a `Policy` attribute: JSON, with at least one statement, each with an `Effect` and a `Resource`. A statement need not
/// name a principal.
pub(crate) fn validate(raw: &str) -> Result<(), ApiError> {
    let doc: Value = serde_json::from_str(raw).map_err(|_| invalid_policy(None))?;
    let statements = doc
        .get("Statement")
        .and_then(Value::as_array)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid_policy(None))?;
    for s in statements {
        if !matches!(
            s.get("Effect").and_then(Value::as_str),
            Some("Allow" | "Deny")
        ) {
            return Err(invalid_policy(None));
        }
        if s.get("Resource").is_none() {
            return Err(invalid_policy(Some("Missing required field Resource.")));
        }
    }
    Ok(())
}

impl Queue {
    /// The statements of this queue's policy.
    fn statements(&self) -> Vec<Value> {
        let doc: Option<Value> = self
            .stored
            .get("Policy")
            .and_then(|p| serde_json::from_str(p).ok());
        doc.and_then(|d| d.get("Statement").and_then(Value::as_array).cloned())
            .unwrap_or_default()
    }

    /// Writes the statements back as the queue's policy; no statements, no policy.
    fn store_statements(&mut self, arn: &str, statements: Vec<Value>) {
        if statements.is_empty() {
            self.stored.remove("Policy");
        } else {
            let doc = json!({"Version": "2012-10-17", "Id": format!("{arn}/SQSDefaultPolicy"), "Statement": statements});
            self.stored.insert("Policy".to_owned(), doc.to_string());
        }
    }

    /// Adds a statement granting `accounts` the `actions`, as `AddPermission` does.
    pub(crate) fn add_permission(
        &mut self,
        arn: &str,
        label: &str,
        accounts: &[&str],
        actions: &[&str],
    ) -> Result<(), ApiError> {
        let mut statements = self.statements();
        if statements
            .iter()
            .any(|s| s.get("Sid").and_then(Value::as_str) == Some(label))
        {
            return Err(invalid_parameter(format!(
                "Value {label} for parameter Label is invalid. Reason: Already exists."
            )));
        }
        if actions.len() > MAX_ACTIONS {
            return Err(ApiError::new(
                403,
                "OverLimit",
                format!(
                    "{} Actions were found, maximum allowed is {MAX_ACTIONS}. ",
                    actions.len()
                ),
            ));
        }
        let named: Vec<String> = actions
            .iter()
            .map(|a| check_action(a))
            .collect::<Result<_, _>>()?;
        if let Some(bad) = accounts
            .iter()
            .find(|a| !(a.len() == 12 && a.bytes().all(|b| b.is_ascii_digit())))
        {
            return Err(invalid_parameter(format!(
                "Value [{bad}] for parameter PrincipalId is invalid. Reason: Unable to verify."
            )));
        }
        if statements.len() >= MAX_STATEMENTS {
            // Unrecorded wording.
            return Err(ApiError::new(
                403,
                "OverLimit",
                format!(
                    "The policy already has {MAX_STATEMENTS} statements, the most it may have."
                ),
            ));
        }
        let principals: Vec<String> = accounts
            .iter()
            .map(|a| format!("arn:aws:iam::{a}:root"))
            .collect();
        statements.push(json!({"Sid": label, "Effect": "Allow", "Principal": {"AWS": principals}, "Action": named, "Resource": arn}));
        self.store_statements(arn, statements);
        Ok(())
    }

    /// Removes the statement `label` names.
    pub(crate) fn remove_permission(&mut self, arn: &str, label: &str) -> Result<(), ApiError> {
        let statements = self.statements();
        let kept: Vec<Value> = statements
            .iter()
            .filter(|s| s.get("Sid").and_then(Value::as_str) != Some(label))
            .cloned()
            .collect();
        if kept.len() == statements.len() {
            return Err(invalid_parameter(format!(
                "Value {label} for parameter Label is invalid. Reason: can't find label on existing policy."
            )));
        }
        self.store_statements(arn, kept);
        Ok(())
    }
}

/// An action as it appears in a policy (`SQS:SendMessage`), or the reason it cannot be granted.
fn check_action(action: &str) -> Result<String, ApiError> {
    let named = format!("SQS:{action}");
    if SHAREABLE.contains(&action) {
        Ok(named)
    } else if OWNER_ONLY.contains(&action) {
        Err(invalid_parameter(format!(
            "Value {named} for parameter ActionName is invalid. Reason: Only the queue owner is allowed to invoke this action."
        )))
    } else {
        Err(invalid_parameter(format!(
            "Value {named} for parameter ActionName is invalid. Reason: Please refer to the appropriate WSDL for a list of valid actions."
        )))
    }
}

impl Core {
    pub(crate) fn add_permission(&mut self, call: &Call) -> Reply {
        let v = request::<Add>(call)?;
        let queue_url = v.get(Add::QUEUE_URL).ok_or_else(|| missing("QueueUrl"))?;
        let label = v.get(Add::LABEL).ok_or_else(|| missing("Label"))?;
        let accounts: Vec<&str> = v
            .get(Add::AWS_ACCOUNT_IDS)
            .ok_or_else(|| missing("AWSAccountIds"))?
            .collect();
        let actions: Vec<&str> = v
            .get(Add::ACTIONS)
            .ok_or_else(|| missing("Actions"))?
            .collect();
        let arn = self.arn_of(call, queue_url)?;
        self.queue_mut(call, queue_url)?
            .add_permission(&arn, label, &accounts, &actions)?;
        empty()
    }

    pub(crate) fn remove_permission(&mut self, call: &Call) -> Reply {
        let v = request::<Remove>(call)?;
        let queue_url = v
            .get(Remove::QUEUE_URL)
            .ok_or_else(|| missing("QueueUrl"))?;
        let label = v.get(Remove::LABEL).ok_or_else(|| missing("Label"))?;
        let arn = self.arn_of(call, queue_url)?;
        self.queue_mut(call, queue_url)?
            .remove_permission(&arn, label)?;
        empty()
    }

    /// The ARN of the queue a URL names.
    pub(crate) fn arn_of(&mut self, call: &Call, queue_url: &str) -> Result<String, ApiError> {
        let name = self.queue_mut(call, queue_url)?.name.clone();
        Ok(arn(&call.caller.region, &call.caller.account_id, &name))
    }
}

/// A queue's ARN.
pub(crate) fn arn(region: &str, account: &str, name: &str) -> String {
    format!("arn:aws:sqs:{region}:{account}:{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_policy_needs_statements_with_an_effect_and_a_resource() {
        let ok = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"sqs:*","Resource":"*"}]}"#;
        assert!(validate(ok).is_ok(), "no principal is fine");
        for bad in [
            "not json",
            r#"{"Statement":[]}"#,
            r#"{"Statement":[{"Principal":"*","Action":"sqs:*","Resource":"*"}]}"#,
        ] {
            assert_eq!(
                validate(bad).unwrap_err().message,
                "Invalid value for the parameter Policy.",
                "{bad}"
            );
        }
        let no_resource = r#"{"Statement":[{"Effect":"Allow","Principal":"*","Action":"sqs:*"}]}"#;
        assert_eq!(
            validate(no_resource).unwrap_err().message,
            "Invalid value for the parameter Policy. Reason: Missing required field Resource."
        );
    }

    #[test]
    fn actions_are_shared_owner_only_or_unknown() {
        assert_eq!(check_action("SendMessage").unwrap(), "SQS:SendMessage");
        assert!(
            check_action("SetQueueAttributes")
                .unwrap_err()
                .message
                .ends_with("Only the queue owner is allowed to invoke this action.")
        );
        assert!(
            check_action("DoEverything")
                .unwrap_err()
                .message
                .contains("WSDL")
        );
    }
}
