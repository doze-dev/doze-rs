//! DeleteTopics.

use std::sync::Arc;

use kafka_auth::authz::operation;
use kafka_meta::Topic;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::ids::{TopicId, TopicName};
use kafka_wire::messages::delete_topics_response::DeletableTopicResult;
use kafka_wire::messages::{DeleteTopicsRequest, DeleteTopicsResponse, TopicName as WireName};

use crate::guard::TOPIC_DENIED;
use crate::{Broker, Caller, blocking};

/// `Errors.UNKNOWN_TOPIC_ID`'s message, which Kafka sends with the code.
const UNKNOWN_ID_WORDS: &str = "This server does not host this topic ID.";

/// A topic a request names, by name (every version) or by id (v6 and later).
struct Target<'a> {
    name: Option<&'a str>,
    id: kafka_wire::Uuid,
}

impl<F: Fs> Broker<F> {
    /// Deletes each named topic: out of the registry first, then its data from disk. The caller must be allowed to
    /// DELETE the topic (and so to DESCRIBE it, which DELETE implies, but a DENY of DESCRIBE is not undone by it):
    /// see [`Broker::delete_denial`] for what a refusal tells.
    pub(crate) async fn delete_topics(
        &self,
        caller: &Caller,
        req: &DeleteTopicsRequest,
    ) -> DeleteTopicsResponse {
        // Until its partitions are cleared the name is not to be made again: the new topic's would be named the same.
        let _one_change_at_a_time = self.topic_admin.lock().await;
        // DELETE on the cluster covers every topic, for describing and for deleting (`ControllerApis.deleteTopics`).
        let cluster = self.cluster_allows(caller, operation::DELETE);
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
            resp.responses
                .push(self.delete_one(caller, cluster, &target).await);
        }
        resp
    }

    async fn delete_one(
        &self,
        caller: &Caller,
        cluster: bool,
        target: &Target<'_>,
    ) -> DeletableTopicResult {
        let found = self.resolve(target);
        if let Some(denied) = self.delete_denial(caller, cluster, target, found.as_deref()) {
            return denied;
        }
        let mut r = DeletableTopicResult::default();
        r.topic_id = found.as_ref().map_or(target.id, |t| t.id.to_uuid());
        r.name = target
            .name
            .map(str::to_owned)
            .or_else(|| found.as_ref().map(|t| t.name.to_string()))
            .map(|n| WireName(StrBytes::from_string(n)));
        let Some(topic) = found else {
            // By id Kafka says so in its own words (`ReplicationControlManager.deleteTopic`); by name it is a topic-partition
            // that is not hosted. (Neither a name nor an id is INVALID_REQUEST there: SEC-D17.)
            if target.name.is_none() && !target.id.is_nil() {
                r.error_code = ResponseError::UnknownTopicId.code();
                r.error_message = Some(StrBytes::from_static_str(UNKNOWN_ID_WORDS));
            } else {
                r.error_code = ResponseError::UnknownTopicOrPartition.code();
            }
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
        self.groups.topics_changed().await;
        r
    }

    /// The answer for a target the caller may not delete, if it may not (`ControllerApis.deleteTopics`, which asks
    /// before it looks for the topic, so that a name that is not there is hidden like one that is):
    ///
    /// - by name, and the caller may not DESCRIBE it: 29 with the name, whether the topic exists or not;
    /// - by name, describable and present, but not deletable: 29 with the name (and no id, as Kafka answers);
    /// - by id, present and not deletable: 29 with the id, and the name only if the caller may DESCRIBE it (an id
    ///   is no secret; the name of a topic one may not describe is).
    ///
    /// A topic that is absent and describable is answered as absent.
    fn delete_denial(
        &self,
        caller: &Caller,
        cluster: bool,
        target: &Target<'_>,
        found: Option<&Topic>,
    ) -> Option<DeletableTopicResult> {
        let name = target.name.or_else(|| found.map(|t| t.name.as_ref()))?;
        let allows = |op| cluster || self.topic_allows(caller, name, op);
        let (describable, deletable) = (allows(operation::DESCRIBE), allows(operation::DELETE));
        let denied = |shown: Option<&str>, id| {
            let mut r = DeletableTopicResult::default();
            r.name = shown.map(|n| WireName(StrBytes::from_string(n.to_owned())));
            r.topic_id = id;
            r.error_code = ResponseError::TopicAuthorizationFailed.code();
            r.error_message = Some(StrBytes::from_static_str(TOPIC_DENIED));
            Some(r)
        };
        if target.name.is_some() {
            let refused = !describable || (found.is_some() && !deletable);
            return refused
                .then(|| denied(Some(name), kafka_wire::Uuid::nil()))
                .flatten();
        }
        if deletable {
            return None;
        }
        denied(describable.then_some(name), target.id)
    }

    /// The topic a target names, if there is one. A name that could not be a topic's is not one.
    fn resolve(&self, target: &Target<'_>) -> Option<Arc<Topic>> {
        match target.name {
            Some(name) => self.meta.topic(&TopicName::parse(name).ok()?),
            None => self.meta.topic_by_id(TopicId::from_uuid(target.id)?),
        }
    }
}
