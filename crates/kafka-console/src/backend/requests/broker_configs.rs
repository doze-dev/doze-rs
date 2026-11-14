//! The requests of the Settings page: the configs of the broker with where each value comes from, a change to one of
//! them (IncrementalAlterConfigs on the BROKER resource), and the topics' own values of the configs a broker default
//! lies under.

use kafka_wire::StrBytes;
use kafka_wire::api::Request;
use kafka_wire::messages::describe_configs_request::DescribeConfigsResource;
use kafka_wire::messages::incremental_alter_configs_request::{
    AlterConfigsResource, AlterableConfig,
};
use kafka_wire::messages::{DescribeConfigsRequest, IncrementalAlterConfigsRequest};

/// The resource type of a topic in the protocol.
const TOPIC: i8 = 2;
/// The resource type of a broker.
pub const BROKER: i8 = 4;

/// The most topics one look at "who sees it" asks about.
pub const MOST_TOPICS: usize = 200;

/// Every config of the broker `node`, with the places each value could come from.
#[must_use]
pub fn describe_broker(node: i32) -> Request {
    let mut res = DescribeConfigsResource::default();
    res.resource_type = BROKER;
    res.resource_name = StrBytes::from_string(node.to_string());
    res.configuration_keys = None;
    let mut r = DescribeConfigsRequest::default();
    r.resources = vec![res];
    r.include_synonyms = true;
    r.include_documentation = false;
    Request::DescribeConfigs(r)
}

/// SET `key` to `value` on the broker resource named `name` (`""` is all brokers, a node id is that broker). With
/// `validate_only` every check runs and nothing is changed.
#[must_use]
pub fn alter_broker(name: &str, key: &str, value: &str, validate_only: bool) -> Request {
    let mut c = AlterableConfig::default();
    c.name = StrBytes::from_string(key.to_owned());
    c.config_operation = 0;
    c.value = Some(StrBytes::from_string(value.to_owned()));
    let mut res = AlterConfigsResource::default();
    res.resource_type = BROKER;
    res.resource_name = StrBytes::from_string(name.to_owned());
    res.configs = vec![c];
    let mut r = IncrementalAlterConfigsRequest::default();
    r.resources = vec![res];
    r.validate_only = validate_only;
    Request::IncrementalAlterConfigs(r)
}

/// These topics' values of `keys`, with no synonyms and no documentation. At most [`MOST_TOPICS`] topics.
#[must_use]
pub fn describe_topics(names: &[String], keys: &[&str]) -> Request {
    let mut r = DescribeConfigsRequest::default();
    r.include_synonyms = false;
    r.include_documentation = false;
    r.resources = names
        .iter()
        .take(MOST_TOPICS)
        .map(|n| {
            let mut res = DescribeConfigsResource::default();
            res.resource_type = TOPIC;
            res.resource_name = StrBytes::from_string(n.clone());
            res.configuration_keys = Some(
                keys.iter()
                    .map(|k| StrBytes::from_string((*k).to_owned()))
                    .collect(),
            );
            res
        })
        .collect();
    Request::DescribeConfigs(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_broker_describe_names_the_node_and_wants_the_synonyms() {
        let Request::DescribeConfigs(r) = describe_broker(7) else {
            panic!("not DescribeConfigs")
        };
        assert_eq!(r.resources[0].resource_type, 4);
        assert_eq!(r.resources[0].resource_name.as_str(), "7");
        assert!(r.include_synonyms && r.resources[0].configuration_keys.is_none());
    }

    #[test]
    fn an_alter_is_one_set_on_the_named_broker_and_validate_only_is_kept() {
        let Request::IncrementalAlterConfigs(r) =
            alter_broker("", "log.retention.ms", "259200000", true)
        else {
            panic!("not IncrementalAlterConfigs")
        };
        assert!(r.validate_only);
        let res = &r.resources[0];
        assert_eq!((res.resource_type, res.resource_name.as_str()), (4, ""));
        assert_eq!(res.configs.len(), 1);
        let c = &res.configs[0];
        assert_eq!(
            (c.name.as_str(), c.config_operation),
            ("log.retention.ms", 0)
        );
        assert_eq!(c.value.as_ref().map(|v| v.as_str()), Some("259200000"));
        let Request::IncrementalAlterConfigs(r) = alter_broker("1", "x", "1", false) else {
            panic!()
        };
        assert!(!r.validate_only);
    }

    #[test]
    fn the_topics_asked_about_are_capped_and_the_keys_are_the_callers() {
        let names: Vec<String> = (0..300).map(|i| format!("t{i}")).collect();
        let Request::DescribeConfigs(r) =
            describe_topics(&names, &["retention.ms", "cleanup.policy"])
        else {
            panic!("not DescribeConfigs")
        };
        assert_eq!(r.resources.len(), MOST_TOPICS);
        assert_eq!(r.resources[0].configuration_keys.as_ref().unwrap().len(), 2);
    }
}
