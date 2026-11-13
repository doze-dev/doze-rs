//! What a request asks to do, for the authorizer: the IAM action (`SNS:Publish`, spelled with upper-case `SNS` as AWS words it in a
//! refusal, recorded 2026-11-09), the ARN acted on, and the topic's own policy. A topic's policy also supplies `AWS:SourceOwner`, the key
//! the default policy hangs on: the account of the caller.

use aws_model_sns::Operation;
use aws_peers::Need;
use aws_wire::service::Call;
use serde_json::Value;

use crate::arn;
use crate::core::Core;

/// The action an operation is authorized as: a batch publish is a publish.
fn action_of(op: Operation) -> String {
    let name = format!("{op:?}");
    if name == "PublishBatch" {
        "Publish".to_owned()
    } else {
        name
    }
}

/// The need of a mobile-push operation: the application or endpoint it names (a new application's ARN is made from its name).
fn platform_need<'a>(
    op: Operation,
    action: &str,
    region: &str,
    account: &str,
    text: &dyn Fn(&str) -> &'a str,
) -> Option<Need> {
    let resource = match op {
        Operation::CreatePlatformApplication => {
            arn::application(region, account, text("Platform"), text("Name"))
        }
        Operation::CreatePlatformEndpoint
        | Operation::DeletePlatformApplication
        | Operation::GetPlatformApplicationAttributes
        | Operation::SetPlatformApplicationAttributes
        | Operation::ListEndpointsByPlatformApplication => {
            text("PlatformApplicationArn").to_owned()
        }
        Operation::DeleteEndpoint
        | Operation::GetEndpointAttributes
        | Operation::SetEndpointAttributes => text("EndpointArn").to_owned(),
        _ => return None,
    };
    Some(Need::new(action, resource))
}

impl Core {
    /// Every thing `call` asks to do. Empty for a request that is not understood (it is refused on its own terms).
    pub(crate) fn needs(&self, call: &Call) -> Vec<Need> {
        let Some(op) = Operation::from_index(call.op) else {
            return Vec::new();
        };
        let body: Value = serde_json::from_str(&call.body).unwrap_or(Value::Null);
        let text = |key: &str| body.get(key).and_then(Value::as_str).unwrap_or_default();
        let (region, account) = (&call.caller.region, &call.caller.account_id);
        let action = format!("SNS:{}", action_of(op));
        // A topic by the ARN a request gives: its policy, if the topic is there.
        let on_topic = |action: &str, topic_arn: &str| {
            let policy = arn::topic_name(topic_arn, "", region, account)
                .ok()
                .flatten()
                .and_then(|n| self.topics.get(n))
                .map(crate::model::Topic::policy);
            Need::new(action, topic_arn)
                .with_policy(policy)
                .with("AWS:SourceOwner", vec![account.clone()])
        };
        if let Some(need) = platform_need(op, &action, region, account, &text) {
            return vec![need];
        }
        let anything = || vec![Need::new(action.clone(), "*")];
        let mut needs = match op {
            Operation::CreateTopic => vec![Need::new(
                action.clone(),
                arn::topic(region, account, text("Name")),
            )],
            Operation::ListTopics
            | Operation::ListSubscriptions
            | Operation::ListPlatformApplications
            | Operation::SetSMSAttributes
            | Operation::GetSMSAttributes
            | Operation::CheckIfPhoneNumberIsOptedOut
            | Operation::OptInPhoneNumber
            | Operation::ListPhoneNumbersOptedOut => anything(),
            Operation::Publish => self.publish_need(&body, &action, &on_topic),
            Operation::Unsubscribe
            | Operation::GetSubscriptionAttributes
            | Operation::SetSubscriptionAttributes => {
                let sub = text("SubscriptionArn");
                match arn::topic_of_subscription(sub) {
                    Some(topic) => vec![on_topic(&action, topic)],
                    None => anything(),
                }
            }
            Operation::TagResource
            | Operation::UntagResource
            | Operation::ListTagsForResource
            | Operation::PutDataProtectionPolicy
            | Operation::GetDataProtectionPolicy => vec![on_topic(&action, text("ResourceArn"))],
            _ => vec![on_topic(&action, text("TopicArn"))],
        };
        // Tags given at creation are a second thing to ask for.
        if op == Operation::CreateTopic
            && body
                .get("Tags")
                .is_some_and(|t| t.as_array().is_some_and(|a| !a.is_empty()))
        {
            needs.push(Need::new(
                "SNS:TagResource",
                arn::topic(region, account, text("Name")),
            ));
        }
        needs
    }

    /// `Publish` is on a topic, a device endpoint or a phone number.
    fn publish_need(
        &self,
        body: &Value,
        action: &str,
        on_topic: &dyn Fn(&str, &str) -> Need,
    ) -> Vec<Need> {
        let text = |key: &str| {
            body.get(key)
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty())
        };
        let target = text("TopicArn").or(text("TargetArn"));
        match target {
            Some(t) if t.contains(":endpoint/") => vec![Need::new(action, t)],
            Some(t) => vec![on_topic(action, t)],
            None => vec![Need::new(action, "*")],
        }
    }
}
