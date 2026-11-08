//! DeleteTopics.

use std::sync::Arc;

use kafka_meta::Topic;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::ids::{TopicId, TopicName};
use kafka_wire::messages::delete_topics_response::DeletableTopicResult;
use kafka_wire::messages::{DeleteTopicsRequest, DeleteTopicsResponse, TopicName as WireName};

use crate::{Broker, blocking};

/// A topic a request names, by name (every version) or by id (v6 and later).
struct Target<'a> {
    name: Option<&'a str>,
    id: kafka_wire::Uuid,
}

impl<F: Fs> Broker<F> {
    /// Deletes each named topic: out of the registry first, then its data from disk.
    pub(crate) async fn delete_topics(&self, req: &DeleteTopicsRequest) -> DeleteTopicsResponse {
        // Until its partitions are cleared the name is not to be made again: the new topic's would be named the same.
        let _one_change_at_a_time = self.topic_admin.lock().await;
        let by_name = req.topic_names.iter().map(|n| Target {
            name: Some(&n.0),
            id: kafka_wire::Uuid::nil(),
        });
        let by_state = req.topics.iter().map(|t| Target {
            name: t.name.as_ref().map(|n| &*n.0),
            id: t.topic_id,
        });
        let mut resp = DeleteTopicsResponse::default();
        for target in by_name.chain(by_state) {
            resp.responses.push(self.delete_one(&target).await);
        }
        resp
    }

    async fn delete_one(&self, target: &Target<'_>) -> DeletableTopicResult {
        let found = self.resolve(target);
        let mut r = DeletableTopicResult::default();
        r.topic_id = found.as_ref().map_or(target.id, |t| t.id.to_uuid());
        r.name = target
            .name
            .map(str::to_owned)
            .or_else(|| found.as_ref().map(|t| t.name.to_string()))
            .map(|n| WireName(StrBytes::from_string(n)));
        let Some(topic) = found else {
            r.error_code = ResponseError::UnknownTopicOrPartition.code();
            return r;
        };
        // The registry first, so that nothing new opens the topic's partitions; then the partitions that are open
        // finish and close; then their files can go.
        if let Err(e) = blocking::run(|| self.meta.delete_topic(&topic.name)) {
            r.error_code = ResponseError::KafkaStorageError.code();
            r.error_message = Some(StrBytes::from_string(e.to_string()));
            return r;
        }
        self.partitions.close_topic(topic.id).await;
        blocking::run(|| self.clear_topic_data(&topic));
        // Fetches waiting on the topic have an answer now: it is gone.
        self.topics_changed.send_modify(|n| *n += 1);
        r
    }

    /// The topic a target names, if there is one. A name that could not be a topic's is not one.
    fn resolve(&self, target: &Target<'_>) -> Option<Arc<Topic>> {
        match target.name {
            Some(name) => self.meta.topic(&TopicName::parse(name).ok()?),
            None => self.meta.topic_by_id(TopicId::from_uuid(target.id)?),
        }
    }
}
