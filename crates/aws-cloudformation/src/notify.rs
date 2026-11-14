//! A stack's `NotificationARNs`: each of its events is published to those SNS topics, in the key='value' lines CloudFormation sends
//! (memory: the message layout and the subject are from AWS's documentation of stack notifications).

use serde_json::json;

use crate::core::Core;
use crate::model::{Event, Stack};

impl Stack {
    /// The account a stack belongs to (from its id).
    pub(crate) fn caller_account(&self) -> String {
        self.id.split(':').nth(4).unwrap_or_default().to_owned()
    }
}

/// The message for one event.
pub(crate) fn message(e: &Event, account: &str) -> String {
    let line = |k: &str, v: &str| format!("{k}='{v}'\n");
    let mut out = String::new();
    out.push_str(&line("StackId", &e.stack_id));
    out.push_str(&line("Timestamp", &crate::progress::iso_ms(e.at_ms)));
    out.push_str(&line("EventId", &e.id));
    out.push_str(&line("LogicalResourceId", &e.logical));
    out.push_str(&line("Namespace", account));
    out.push_str(&line("PhysicalResourceId", &e.physical));
    out.push_str(&line("PrincipalId", account));
    out.push_str(&line(
        "ResourceProperties",
        e.props.as_deref().unwrap_or("null"),
    ));
    out.push_str(&line("ResourceStatus", &e.status));
    out.push_str(&line(
        "ResourceStatusReason",
        e.reason.as_deref().unwrap_or(""),
    ));
    out.push_str(&line("ResourceType", &e.ty));
    out.push_str(&line("StackName", &e.stack_name));
    out.push_str(&line(
        "ClientRequestToken",
        e.token.as_deref().unwrap_or("null"),
    ));
    out
}

impl Core {
    /// Publishes what the outbox holds. A topic that is gone, or SNS not running, loses the message (as AWS would), and the log
    /// says so.
    pub(crate) async fn notify(&mut self) {
        let outbox = std::mem::take(&mut self.outbox);
        let Some(peers) = self.peers.clone() else {
            return;
        };
        for (arn, message) in outbox {
            let region = arn.split(':').nth(3).unwrap_or_default().to_owned();
            let caller = aws_wire::service::Caller {
                access_key_id: "cloudformation".into(),
                account_id: arn.split(':').nth(4).unwrap_or_default().to_owned(),
                region,
                endpoint: String::new(),
                service: Some("cloudformation".into()),
                session_token: None,
            };
            let body = json!({"TopicArn": arn, "Subject": "AWS CloudFormation Notification", "Message": message});
            if let Err(e) = peers
                .call("cloudformation", &caller, "sns", "Publish", &body)
                .await
            {
                tracing::warn!(
                    "cloudformation: notification to {arn} not delivered: {}",
                    e.message
                );
            }
        }
    }
}
