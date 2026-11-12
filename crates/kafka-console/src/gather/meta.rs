//! The topics of the broker and their partitions, from Metadata.

use kafka_wire::Uuid;
use kafka_wire::api::Response;

use crate::backend::{Backend, requests};

/// A topic as Metadata names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TopicMeta {
    pub name: String,
    pub id: Uuid,
    /// Its partition numbers.
    pub partitions: Vec<i32>,
    pub internal: bool,
}

fn from(t: &kafka_wire::messages::metadata_response::MetadataResponseTopic) -> Option<TopicMeta> {
    if t.error_code != 0 {
        return None;
    }
    Some(TopicMeta {
        name: t.name.as_ref()?.0.to_string(),
        id: t.topic_id,
        partitions: t.partitions.iter().map(|p| p.partition_index).collect(),
        internal: t.is_internal,
    })
}

/// Every topic the console may describe, by name.
pub(super) async fn all(b: &Backend) -> Option<Vec<TopicMeta>> {
    let Ok(Response::Metadata(m)) = b.call(requests::metadata_all()).await else {
        return None;
    };
    let mut out: Vec<TopicMeta> = m.topics.iter().filter_map(from).collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Some(out)
}

/// One topic, if it exists and the console may describe it.
pub(super) async fn one(b: &Backend, name: &str) -> Option<TopicMeta> {
    let Ok(Response::Metadata(m)) = b.call(requests::metadata_topics(&[name.to_owned()])).await
    else {
        return None;
    };
    m.topics.iter().find_map(from)
}
