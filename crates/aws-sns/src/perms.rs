//! `AddPermission` and `RemovePermission`: statements added to, and taken from, a topic's policy. The wording of the refusals is from
//! memory; AWS also checks that an account exists, which a laptop cannot, so any 12-digit id passes.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::req::{Req, invalid};

/// The actions a statement of a topic's policy may name.
const ACTIONS: &[&str] = &[
    "AddPermission",
    "DeleteTopic",
    "GetTopicAttributes",
    "ListSubscriptionsByTopic",
    "Publish",
    "Receive",
    "RemovePermission",
    "SetTopicAttributes",
    "Subscribe",
];

fn is_account(id: &str) -> bool {
    id.len() == 12 && id.bytes().all(|b| b.is_ascii_digit())
}

/// The statement a permission adds.
fn statement(label: &str, topic_arn: &str, accounts: &[String], actions: &[String]) -> Value {
    let principals: Vec<String> = accounts
        .iter()
        .map(|a| format!("arn:aws:iam::{a}:root"))
        .collect();
    let principal = match principals.as_slice() {
        [one] => json!(one),
        many => json!(many),
    };
    let actions: Vec<String> = actions.iter().map(|a| format!("SNS:{a}")).collect();
    json!({
        "Sid": label,
        "Effect": "Allow",
        "Principal": {"AWS": principal},
        "Action": actions,
        "Resource": topic_arn
    })
}

impl Core {
    /// `AddPermission`.
    pub(crate) fn add_permission(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let key = self.topic_key(r.s("TopicArn"), "TopicArn", r)?;
        let (label, accounts, actions) = (
            r.s("Label"),
            r.strings("AWSAccountId"),
            r.strings("ActionName"),
        );
        if label.is_empty() || label.len() > 256 {
            return Err(invalid("Label"));
        }
        if accounts.is_empty() || !accounts.iter().all(|a| is_account(a)) {
            return Err(invalid("Account ID"));
        }
        if actions.is_empty()
            || !actions
                .iter()
                .all(|a| ACTIONS.contains(&a.as_str()) || a == "*")
        {
            return Err(invalid("Policy statement action out of service scope!"));
        }
        let Some(topic) = self.topics.get_mut(&key) else {
            return Ok(Value::Null);
        };
        let mut policy: Value = serde_json::from_str(&topic.policy()).unwrap_or_else(|_| json!({}));
        let Some(statements) = statements_of(&mut policy) else {
            return Err(invalid("Policy"));
        };
        if statements.iter().any(|s| s["Sid"] == label) {
            return Err(invalid("Statement already exists"));
        }
        statements.push(statement(label, &topic.arn, &accounts, &actions));
        topic.attrs.insert("Policy".into(), policy.to_string());
        Ok(Value::Null)
    }

    /// `RemovePermission`.
    pub(crate) fn remove_permission(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let key = self.topic_key(r.s("TopicArn"), "TopicArn", r)?;
        let label = r.s("Label");
        let Some(topic) = self.topics.get_mut(&key) else {
            return Ok(Value::Null);
        };
        let mut policy: Value = serde_json::from_str(&topic.policy()).unwrap_or_else(|_| json!({}));
        let Some(statements) = statements_of(&mut policy) else {
            return Err(invalid("Policy"));
        };
        let before = statements.len();
        statements.retain(|s| s["Sid"] != label);
        if statements.len() == before {
            return Err(invalid("Statement does not exist"));
        }
        topic.attrs.insert("Policy".into(), policy.to_string());
        Ok(Value::Null)
    }
}

/// The `Statement` array of a policy, made if it is missing or is a single statement.
fn statements_of(policy: &mut Value) -> Option<&mut Vec<Value>> {
    if !policy["Statement"].is_array() {
        let single = policy["Statement"].take();
        policy["Statement"] = if single.is_null() {
            json!([])
        } else {
            json!([single])
        };
    }
    policy.get_mut("Statement")?.as_array_mut()
}
