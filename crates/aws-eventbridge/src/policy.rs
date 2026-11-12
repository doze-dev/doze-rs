//! Asking a resource's own policy whether `events.amazonaws.com` may act on it. EventBridge has no right of its own over a queue or a
//! topic: with no policy, or none that names it, delivery is refused.

use aws_iam_policy::{Context, Policies, Request, Subject};

/// Whether `policy` lets `events.amazonaws.com` do `action` to `resource`, for the rule `source_arn` of `source_account`.
pub(crate) fn allows(
    policy: Option<&str>,
    action: &str,
    resource: &str,
    source_arn: &str,
    source_account: &str,
) -> bool {
    let Some(Ok(doc)) = policy.and_then(aws_iam_policy::parse) else {
        return false;
    };
    let mut context = Context::new();
    context
        .set("aws:SourceArn", source_arn.to_owned())
        .set("aws:SourceAccount", source_account.to_owned())
        .set("aws:SourceOwner", source_account.to_owned())
        .set("aws:PrincipalServiceName", "events.amazonaws.com");
    let subject = Subject {
        arns: Vec::new(),
        account: source_account.to_owned(),
        service: Some("events.amazonaws.com".to_owned()),
    };
    let resource_policies = [&doc];
    let policies = Policies {
        resource: Some(&resource_policies),
        ..Policies::default()
    };
    let request = Request {
        action,
        resource,
        context: &context,
        subject: &subject,
    };
    aws_iam_policy::evaluate(&policies, &request).allowed()
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUEUE: &str = "arn:aws:sqs:us-east-1:111122223333:q";
    const RULE: &str = "arn:aws:events:us-east-1:111122223333:rule/bus/r";

    fn doc(condition: &str) -> String {
        format!(
            r#"{{"Version":"2012-10-17","Statement":[{{"Effect":"Allow","Principal":{{"Service":"events.amazonaws.com"}},"Action":"sqs:SendMessage","Resource":"{QUEUE}"{condition}}}]}}"#
        )
    }

    #[test]
    fn the_queue_policy_of_the_conformance_scenario_lets_its_rule_deliver_and_no_other() {
        let by_rule = doc(&format!(
            r#","Condition":{{"ArnEquals":{{"aws:SourceArn":"{RULE}"}}}}"#
        ));
        assert!(allows(
            Some(&by_rule),
            "sqs:SendMessage",
            QUEUE,
            RULE,
            "111122223333"
        ));
        let other = "arn:aws:events:us-east-1:111122223333:rule/bus/other";
        assert!(!allows(
            Some(&by_rule),
            "sqs:SendMessage",
            QUEUE,
            other,
            "111122223333"
        ));
        assert!(allows(
            Some(&doc("")),
            "sqs:SendMessage",
            QUEUE,
            other,
            "111122223333"
        ));
        assert!(!allows(
            None,
            "sqs:SendMessage",
            QUEUE,
            RULE,
            "111122223333"
        ));
        assert!(!allows(
            Some("{not json"),
            "sqs:SendMessage",
            QUEUE,
            RULE,
            "111122223333"
        ));
    }
}
