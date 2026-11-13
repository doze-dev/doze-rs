//! ARNs: the topic, subscription, application and endpoint ARNs SNS makes, and the checks it makes of one a caller gives.

use aws_wire::ApiError;

use crate::req::{invalid, invalid_because};

/// An ARN split at its first five colons; `resource` keeps any colons after them.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Arn<'a> {
    pub(crate) service: &'a str,
    pub(crate) region: &'a str,
    pub(crate) account: &'a str,
    pub(crate) resource: &'a str,
}

/// Splits `text`, or says why it is not an ARN. The first sentence is what AWS answered for `nonsense` (recorded 2026-11-09); the second is
/// from memory.
pub(crate) fn split<'a>(text: &'a str, param: &str) -> Result<Arn<'a>, ApiError> {
    let parts: Vec<&str> = text.splitn(6, ':').collect();
    let [prefix, _partition, service, region, account, resource] = parts[..] else {
        return Err(invalid_because(
            param,
            format!("An ARN must have at least 6 elements, not {}", parts.len()),
        ));
    };
    if prefix != "arn" {
        // (memory)
        return Err(invalid_because(param, "An ARN must start with arn:"));
    }
    Ok(Arn {
        service,
        region,
        account,
        resource,
    })
}

/// The `arn:aws:sns:<region>:<account>:<name>` of a topic.
pub(crate) fn topic(region: &str, account: &str, name: &str) -> String {
    format!("arn:aws:sns:{region}:{account}:{name}")
}

pub(crate) fn subscription(topic_arn: &str, id: &str) -> String {
    format!("{topic_arn}:{id}")
}

pub(crate) fn application(region: &str, account: &str, platform: &str, name: &str) -> String {
    format!("arn:aws:sns:{region}:{account}:app/{platform}/{name}")
}

pub(crate) fn endpoint(region: &str, account: &str, platform: &str, app: &str, id: &str) -> String {
    format!("arn:aws:sns:{region}:{account}:endpoint/{platform}/{app}/{id}")
}

/// The name of the topic a topic ARN names, for the account and region the call is in. An ARN of another region or account names a topic
/// this service does not hold (`None`).
pub(crate) fn topic_name<'a>(
    text: &'a str,
    param: &str,
    region: &str,
    account: &str,
) -> Result<Option<&'a str>, ApiError> {
    let arn = split(text, param)?;
    if arn.service != "sns" || arn.resource.is_empty() {
        // (memory)
        return Err(invalid(param));
    }
    if arn.resource.contains(':') || arn.region != region || arn.account != account {
        return Ok(None);
    }
    Ok(Some(arn.resource))
}

/// The topic ARN a subscription ARN belongs to (its last element is the subscription's id).
pub(crate) fn topic_of_subscription(sub: &str) -> Option<&str> {
    sub.rsplit_once(':').map(|(topic, _)| topic)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_is_not_an_arn_and_the_words_are_aws() {
        let e = split("nonsense", "TopicArn").unwrap_err();
        assert_eq!(
            e.message,
            "Invalid parameter: TopicArn Reason: An ARN must have at least 6 elements, not 1"
        );
        let arn = split("arn:aws:sns:us-east-1:111122223333:t:abc", "x").unwrap();
        assert_eq!(arn.resource, "t:abc");
    }

    #[test]
    fn a_topic_of_another_region_or_account_is_not_held() {
        let here = "arn:aws:sns:us-east-1:111122223333:t";
        assert_eq!(
            topic_name(here, "TopicArn", "us-east-1", "111122223333").unwrap(),
            Some("t")
        );
        assert_eq!(
            topic_name(here, "TopicArn", "eu-west-1", "111122223333").unwrap(),
            None
        );
        assert!(
            topic_name(
                "arn:aws:sqs:us-east-1:111122223333:t",
                "TopicArn",
                "us-east-1",
                "1"
            )
            .is_err()
        );
        assert_eq!(
            topic_of_subscription("arn:aws:sns:us-east-1:1:t:abc"),
            Some("arn:aws:sns:us-east-1:1:t")
        );
    }
}
