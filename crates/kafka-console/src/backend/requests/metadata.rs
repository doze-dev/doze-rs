//! Metadata for named topics.

use kafka_wire::StrBytes;
use kafka_wire::api::Request;
use kafka_wire::messages::metadata_request::MetadataRequestTopic;
use kafka_wire::messages::{MetadataRequest, TopicName};

/// Metadata for these topics by name, creating none (the console never makes a topic by asking about it). At most 200:
/// more than a page of the console shows.
#[must_use]
pub fn metadata_topics(names: &[String]) -> Request {
    let mut r = MetadataRequest::default();
    r.topics = Some(
        names
            .iter()
            .take(200)
            .map(|n| {
                let mut t = MetadataRequestTopic::default();
                t.name = Some(TopicName(StrBytes::from_string(n.clone())));
                t
            })
            .collect(),
    );
    r.allow_auto_topic_creation = false;
    Request::Metadata(r)
}
