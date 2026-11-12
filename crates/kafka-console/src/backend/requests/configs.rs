//! DescribeConfigs for topics: the few facts a topic's page says (cleanup policy, retention, the size of a record).

use kafka_wire::StrBytes;
use kafka_wire::api::Request;
use kafka_wire::messages::DescribeConfigsRequest;
use kafka_wire::messages::describe_configs_request::DescribeConfigsResource;

/// The resource type of a topic in the protocol.
const TOPIC: i8 = 2;

/// The configs the topic pages say something about.
pub const KEYS: [&str; 4] = [
    "cleanup.policy",
    "retention.ms",
    "max.message.bytes",
    "message.timestamp.type",
];

/// DescribeConfigs of these topics for [`KEYS`], with no synonyms or documentation. At most 200 topics.
#[must_use]
pub fn describe_topic_configs(names: &[String]) -> Request {
    let mut r = DescribeConfigsRequest::default();
    r.include_synonyms = false;
    r.include_documentation = false;
    r.resources = names
        .iter()
        .take(200)
        .map(|n| {
            let mut res = DescribeConfigsResource::default();
            res.resource_type = TOPIC;
            res.resource_name = StrBytes::from_string(n.clone());
            res.configuration_keys =
                Some(KEYS.iter().map(|k| StrBytes::from_static_str(k)).collect());
            res
        })
        .collect();
    Request::DescribeConfigs(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_resource_per_topic_asking_only_for_the_keys_the_pages_use() {
        let Request::DescribeConfigs(r) = describe_topic_configs(&["a".to_owned(), "b".to_owned()])
        else {
            panic!("not DescribeConfigs");
        };
        assert_eq!(r.resources.len(), 2);
        assert!(r.resources.iter().all(|x| x.resource_type == 2));
        let keys = r.resources[0].configuration_keys.as_ref().unwrap();
        assert_eq!(keys.len(), KEYS.len());
        assert!(!r.include_synonyms && !r.include_documentation);
    }
}
