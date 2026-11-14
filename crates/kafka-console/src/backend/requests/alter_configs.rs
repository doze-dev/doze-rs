//! The requests of the Config pages: every config of a topic with where each value comes from and Kafka's text, and
//! IncrementalAlterConfigs, which is how a config is set, reset, appended to and subtracted from.

use kafka_wire::StrBytes;
use kafka_wire::api::Request;
use kafka_wire::messages::describe_configs_request::DescribeConfigsResource;
use kafka_wire::messages::incremental_alter_configs_request::{
    AlterConfigsResource, AlterableConfig,
};
use kafka_wire::messages::{DescribeConfigsRequest, IncrementalAlterConfigsRequest};

use crate::model::ConfigOp;

/// The resource type of a topic in the protocol.
const TOPIC: i8 = 2;

/// DescribeConfigs of every config of one topic, with the places each value could come from and Kafka's text for it.
#[must_use]
pub fn describe_all(topic: &str) -> Request {
    let mut res = DescribeConfigsResource::default();
    res.resource_type = TOPIC;
    res.resource_name = StrBytes::from_string(topic.to_owned());
    res.configuration_keys = None;
    let mut r = DescribeConfigsRequest::default();
    r.resources = vec![res];
    r.include_synonyms = true;
    r.include_documentation = true;
    Request::DescribeConfigs(r)
}

/// IncrementalAlterConfigs of one topic: these changes, `(key, operation, value)`, in this order. A delete carries no
/// value. `validate_only` runs every check and changes nothing.
#[must_use]
pub fn alter(
    topic: &str,
    changes: &[(&str, ConfigOp, Option<&str>)],
    validate_only: bool,
) -> Request {
    let mut res = AlterConfigsResource::default();
    res.resource_type = TOPIC;
    res.resource_name = StrBytes::from_string(topic.to_owned());
    res.configs = changes
        .iter()
        .map(|(key, op, value)| {
            let mut c = AlterableConfig::default();
            c.name = StrBytes::from_string((*key).to_owned());
            c.config_operation = op.wire();
            c.value = value.map(|v| StrBytes::from_string(v.to_owned()));
            c
        })
        .collect();
    let mut r = IncrementalAlterConfigsRequest::default();
    r.resources = vec![res];
    r.validate_only = validate_only;
    Request::IncrementalAlterConfigs(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_describe_asks_for_every_config_with_its_places_and_its_text() {
        let Request::DescribeConfigs(r) = describe_all("inventory.levels") else {
            panic!("not DescribeConfigs")
        };
        assert_eq!(r.resources.len(), 1);
        assert_eq!(r.resources[0].resource_type, 2);
        assert_eq!(r.resources[0].resource_name.as_str(), "inventory.levels");
        assert!(
            r.resources[0].configuration_keys.is_none(),
            "no keys is all"
        );
        assert!(r.include_synonyms && r.include_documentation);
    }

    #[test]
    fn an_alter_carries_each_change_with_its_operation_number_and_a_delete_has_no_value() {
        let Request::IncrementalAlterConfigs(r) = alter(
            "t",
            &[
                ("a", ConfigOp::Set, Some("1")),
                ("b", ConfigOp::Delete, None),
                ("c", ConfigOp::Append, Some("x")),
                ("d", ConfigOp::Subtract, Some("y")),
            ],
            true,
        ) else {
            panic!("not IncrementalAlterConfigs")
        };
        assert!(r.validate_only);
        let got: Vec<_> = r.resources[0]
            .configs
            .iter()
            .map(|c| {
                (
                    c.name.as_str(),
                    c.config_operation,
                    c.value.as_ref().map(|v| v.as_str()),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                ("a", 0, Some("1")),
                ("b", 1, None),
                ("c", 2, Some("x")),
                ("d", 3, Some("y"))
            ]
        );
        assert_eq!(r.resources[0].resource_type, 2);
    }
}
