//! Metadata: this broker, and the topics it has.

use kafka_auth::authz::operation;
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
use crate::{Broker, Caller, Listener};

/// Where an entry goes in the answer (`KafkaApis.scala` 983-985). Kafka lists the topic ids nobody knows first, then
/// what the caller may see (in the order asked), then the topics it may describe but not create, then those it may
/// not describe.
#[derive(Clone, Copy)]
enum Place {
    UnknownId,
    Listed,
    CreateDenied,
    DescribeDenied,
}

impl<F: Fs> Broker<F> {
    /// Describes the single-node cluster: this broker is every partition's leader, the controller, and the only
    /// replica. A request that names no topics asks for all of them (a null list; before v1, an empty one).
    ///
    /// Topics a client asked for that do not exist and may be created are returned beside the answer: they are made
    /// after it, and under the lock that serializes topic creation and deletion (see [`Broker::create_wanted`]).
    ///
    /// What `caller` may see is what it may DESCRIBE: a list of all topics holds only those, and a topic asked for
    /// that it may not is answered 29 with its name and nothing else, whether it exists or not. A missing topic is
    /// created for a caller that may CREATE it, on the cluster or on the topic; for one that may not, the answer is 29
    /// and nothing is made.
    ///
    /// From v8 a client may ask what it is allowed to do with each topic it is told of, and (v8 to v10) with the
    /// cluster: see [`Broker::with_operations`].
    pub(crate) fn metadata(
        &self,
        at: &Listener,
        caller: &Caller,
        req: &MetadataRequest,
        version: i16,
    ) -> (MetadataResponse, Vec<TopicName>) {
        let (mut resp, wanted) = self.metadata_topics(at, caller, req, version);
        self.with_operations(caller, req, version, &mut resp);
        (resp, wanted)
    }

    /// The cluster's authorized operations, when they were asked for (`KafkaApis.handleTopicMetadataRequest`, 960 to
    /// 972): 0 if the caller may not DESCRIBE the cluster. A field not asked for stays the schema's default, as does
    /// one the version does not have.
    fn with_operations(
        &self,
        caller: &Caller,
        req: &MetadataRequest,
        version: i16,
        resp: &mut MetadataResponse,
    ) {
        if (8..=10).contains(&version) && req.include_cluster_authorized_operations {
            resp.cluster_authorized_operations = self.cluster_operations(caller);
        }
    }

    /// The topics' authorized operations, when they were asked for (v8 and later; 973 to 980). Only the entries Kafka
    /// built from `topicMetadata` have them: the topics the caller may describe (and, of those, the ones that do not
    /// exist). The entries refused for DESCRIBE or CREATE and the unknown ids are appended afterwards and keep the
    /// schema's default.
    fn with_topic_operations(
        &self,
        caller: &Caller,
        req: &MetadataRequest,
        version: i16,
        listed: &mut [MetadataResponseTopic],
    ) {
        if version < 8 || !req.include_topic_authorized_operations {
            return;
        }
        for t in listed {
            if let Some(name) = &t.name {
                t.topic_authorized_operations = self.topic_operations(caller, &name.0);
            }
        }
    }

    fn metadata_topics(
        &self,
        at: &Listener,
        caller: &Caller,
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
                .filter(|t| self.can_describe(caller, t.name.as_ref()))
                .map(|t| self.described(t))
                .collect();
            self.with_topic_operations(caller, req, version, &mut resp.topics);
            return (resp, Vec::new());
        }
        // Kafka before v4 never said whether a client wanted a topic created: it always did.
        let may_create = req.allow_auto_topic_creation || version < 4;
        let mut wanted = Vec::new();
        let (mut unknown_ids, mut listed) = (Vec::new(), Vec::new());
        let (mut create_denied, mut describe_denied) = (Vec::new(), Vec::new());
        for asked in req.topics.iter().flatten() {
            match self.one_topic(caller, asked, may_create, &mut wanted) {
                (Place::UnknownId, t) => unknown_ids.push(t),
                (Place::Listed, t) => listed.push(t),
                (Place::CreateDenied, t) => create_denied.push(t),
                (Place::DescribeDenied, t) => describe_denied.push(t),
            }
        }
        self.with_topic_operations(caller, req, version, &mut listed);
        resp.topics = unknown_ids;
        resp.topics.extend(listed);
        resp.topics.extend(create_denied);
        resp.topics.extend(describe_denied);
        (resp, wanted)
    }

    fn one_topic(
        &self,
        caller: &Caller,
        asked: &MetadataRequestTopic,
        may_create: bool,
        wanted: &mut Vec<TopicName>,
    ) -> (Place, MetadataResponseTopic) {
        let Some(raw) = asked.name.as_ref().map(|n| &*n.0) else {
            return self.topic_by_id(caller, asked);
        };
        // Before the topic is looked for: a caller that may not describe it is not told whether it is there.
        if !self.can_describe(caller, raw) {
            let denied = failed(Some(raw), ResponseError::TopicAuthorizationFailed);
            return (Place::DescribeDenied, denied);
        }
        let name = match TopicName::parse(raw) {
            Ok(name) => name,
            // The difference between "unknown" and "invalid" tells a client its request can never succeed.
            Err(e) => {
                warn!(topic = raw, why = %name_message(e, raw), "metadata asked for an invalid topic name");
                let invalid = failed(Some(raw), ResponseError::InvalidTopicException);
                return (Place::Listed, invalid);
            }
        };
        if let Some(topic) = self.meta.topic(&name) {
            return (Place::Listed, self.described(&topic));
        }
        if may_create && self.config.auto_create_topics {
            if !self.can_create(caller, raw) {
                let denied = failed(Some(raw), ResponseError::TopicAuthorizationFailed);
                return (Place::CreateDenied, denied);
            }
            wanted.push(name);
        }
        // Kafka creates the topic after it has answered: the request that asked for it is told it does not exist
        // and the next one finds it. Answering with the new topic would hide, from a client that cannot cope
        // with that first answer, what it will meet there.
        let missing = failed(Some(raw), ResponseError::UnknownTopicOrPartition);
        (Place::Listed, missing)
    }

    /// A topic asked for by id. The id of a topic that exists is not a secret, so one the caller may not describe is
    /// answered 29 with the id and no name.
    fn topic_by_id(
        &self,
        caller: &Caller,
        asked: &MetadataRequestTopic,
    ) -> (Place, MetadataResponseTopic) {
        let id = TopicId::from_uuid(asked.topic_id);
        let (place, mut r) = match id.and_then(|id| self.meta.topic_by_id(id)) {
            Some(topic) if self.can_describe(caller, topic.name.as_ref()) => {
                (Place::Listed, self.described(&topic))
            }
            Some(_) => (
                Place::DescribeDenied,
                failed(None, ResponseError::TopicAuthorizationFailed),
            ),
            None => (Place::UnknownId, failed(None, ResponseError::UnknownTopicId)),
        };
        if r.error_code != 0 {
            r.topic_id = asked.topic_id;
        }
        (place, r)
    }

    fn can_describe(&self, caller: &Caller, topic: &str) -> bool {
        self.topic_allows(caller, topic, operation::DESCRIBE)
    }

    /// CREATE on the cluster covers every topic; failing that, CREATE on the topic itself.
    fn can_create(&self, caller: &Caller, topic: &str) -> bool {
        self.cluster_allows(caller, operation::CREATE)
            || self.topic_allows(caller, topic, operation::CREATE)
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
        self.groups.topics_changed().await;
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
