//! Metadata: this broker, and the topics it has.

use kafka_meta::Topic;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::ids::{TopicId, TopicName};
use kafka_wire::messages::metadata_request::MetadataRequestTopic;
use kafka_wire::messages::metadata_response::{
    MetadataResponseBroker, MetadataResponsePartition, MetadataResponseTopic,
};
use kafka_wire::messages::{BrokerId, MetadataRequest, MetadataResponse, TopicName as WireName};
use tracing::warn;

use super::create_topics::name_message;
use crate::{Broker, Listener};

impl<F: Fs> Broker<F> {
    /// Describes the single-node cluster: this broker is every partition's leader, the controller, and the only
    /// replica. A request that names no topics asks for all of them (a null list; before v1, an empty one).
    ///
    /// Topics a client asked for that do not exist and may be created are returned beside the answer: they are made
    /// after it, and under the lock that serializes topic creation and deletion (see [`Broker::create_wanted`]).
    pub(crate) fn metadata(
        &self,
        at: &Listener,
        req: &MetadataRequest,
        version: i16,
    ) -> (MetadataResponse, Vec<TopicName>) {
        let mut resp = MetadataResponse::default();
        let mut me = MetadataResponseBroker::default();
        me.node_id = BrokerId(self.config.node_id);
        me.host = StrBytes::from_string(at.advertised_host.clone());
        me.port = at.advertised_port;
        resp.brokers.push(me);
        resp.cluster_id = Some(StrBytes::from_string(self.config.cluster_id.clone()));
        resp.controller_id = BrokerId(self.config.node_id);

        let all = match &req.topics {
            None => true,
            Some(asked) => asked.is_empty() && version == 0,
        };
        if all {
            resp.topics = self
                .meta
                .topics()
                .iter()
                .map(|t| self.described(t))
                .collect();
            return (resp, Vec::new());
        }
        // Kafka before v4 never said whether a client wanted a topic created: it always did.
        let may_create = req.allow_auto_topic_creation || version < 4;
        let mut wanted = Vec::new();
        for asked in req.topics.iter().flatten() {
            resp.topics
                .push(self.one_topic(asked, may_create, &mut wanted));
        }
        (resp, wanted)
    }

    fn one_topic(
        &self,
        asked: &MetadataRequestTopic,
        may_create: bool,
        wanted: &mut Vec<TopicName>,
    ) -> MetadataResponseTopic {
        let Some(raw) = asked.name.as_ref().map(|n| &*n.0) else {
            return self.topic_by_id(asked);
        };
        let name = match TopicName::parse(raw) {
            Ok(name) => name,
            // The difference between "unknown" and "invalid" tells a client its request can never succeed.
            Err(e) => {
                warn!(topic = raw, why = %name_message(e, raw), "metadata asked for an invalid topic name");
                return failed(Some(raw), ResponseError::InvalidTopicException);
            }
        };
        if let Some(topic) = self.meta.topic(&name) {
            return self.described(&topic);
        }
        if may_create && self.config.auto_create_topics {
            wanted.push(name);
        }
        // Kafka creates the topic after it has answered: the request that asked for it is told it does not exist
        // and the next one finds it. Answering with the new topic would hide, from a client that cannot cope
        // with that first answer, what it will meet there.
        failed(Some(raw), ResponseError::UnknownTopicOrPartition)
    }

    fn topic_by_id(&self, asked: &MetadataRequestTopic) -> MetadataResponseTopic {
        let id = TopicId::from_uuid(asked.topic_id);
        match id.and_then(|id| self.meta.topic_by_id(id)) {
            Some(topic) => self.described(&topic),
            None => {
                let mut r = failed(None, ResponseError::UnknownTopicId);
                r.topic_id = asked.topic_id;
                r
            }
        }
    }

    /// Makes the topics that Metadata requests asked for and found missing, one at a time, under the topic lock. One that
    /// exists by now (a request for it raced) is left as it is.
    pub(crate) async fn create_wanted(&self, names: Vec<TopicName>) {
        if names.is_empty() {
            return;
        }
        let Some(partitions) = kafka_wire::ids::PartitionCount::new(self.config.default_partitions)
        else {
            return;
        };
        let _one_change_at_a_time = self.topic_admin.lock().await;
        for name in names {
            let made = crate::blocking::run(|| {
                self.make_topic(&name, partitions, Default::default(), &Default::default())
            });
            if let Err(e) = made {
                warn!(topic = %name, error = %e, "creating a topic a client asked for failed");
            }
        }
    }

    fn described(&self, topic: &Topic) -> MetadataResponseTopic {
        let node = BrokerId(self.config.node_id);
        let mut r = MetadataResponseTopic::default();
        r.name = Some(WireName(StrBytes::from_string(topic.name.to_string())));
        r.topic_id = topic.id.to_uuid();
        r.partitions = topic
            .partitions
            .ids()
            .map(|p| {
                let mut part = MetadataResponsePartition::default();
                part.partition_index = p.get();
                part.leader_id = node;
                // A single node has one epoch: what ListOffsets answers too.
                part.leader_epoch = 0;
                part.replica_nodes = vec![node];
                part.isr_nodes = vec![node];
                part
            })
            .collect();
        r
    }
}

fn failed(name: Option<&str>, error: ResponseError) -> MetadataResponseTopic {
    let mut r = MetadataResponseTopic::default();
    r.error_code = error.code();
    r.name = name.map(|n| WireName(StrBytes::from_string(n.to_owned())));
    r
}
