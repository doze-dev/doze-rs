//! Requests to the topic APIs as a given caller, for the authorization tests (slice 8d-1).
//!
//! Include with `#[path = "support/authz_topics.rs"] mod topics;` after `mod support;`.
#![allow(
    unreachable_pub,
    dead_code,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::type_complexity
)]

use crate::support::acls::{ALLOW, DENY, LITERAL, PREFIXED, TOPIC, creation};
use crate::support::{Rig, produce_request, records, topic, wire_name};
use kafka_broker::Caller;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::create_acls_request::AclCreation;
use kafka_wire::messages::create_topics_request::CreatableTopic;
use kafka_wire::messages::create_topics_response::CreatableTopicResult;
use kafka_wire::messages::delete_records_request::{DeleteRecordsPartition, DeleteRecordsTopic};
use kafka_wire::messages::delete_topics_response::DeletableTopicResult;
use kafka_wire::messages::describe_configs_request::DescribeConfigsResource;
use kafka_wire::messages::describe_configs_response::DescribeConfigsResult;
use kafka_wire::messages::describe_producers_request::TopicRequest;
use kafka_wire::messages::list_offsets_request::{ListOffsetsPartition, ListOffsetsTopic};
use kafka_wire::messages::produce_response::PartitionProduceResponse;
use kafka_wire::messages::{
    CreateTopicsRequest, DeleteRecordsRequest, DeleteRecordsResponse, DeleteTopicsRequest,
    DescribeConfigsRequest, DescribeProducersRequest, DescribeProducersResponse, FetchResponse,
    ListOffsetsRequest, ListOffsetsResponse, ProduceRequest, ProduceResponse,
};

pub const DELETE: i8 = 6;
pub const CLUSTER_ACTION: i8 = 9;
pub const DESCRIBE_CONFIGS: i8 = 10;
pub const ALTER_CONFIGS: i8 = 11;

pub const TOPIC_AUTHORIZATION_FAILED: i16 = 29;
pub const CLUSTER_AUTHORIZATION_FAILED: i16 = 31;

/// A topic ACL for `principal` on a name, any pattern.
pub fn topic_acl(name: &str, pattern: i8, principal: &str, op: i8, permission: i8) -> AclCreation {
    creation((TOPIC, name, pattern), principal, op, permission)
}

/// ALLOW of `op` on the literal topic `name` for `principal`.
pub fn allow(name: &str, principal: &str, op: i8) -> AclCreation {
    topic_acl(name, LITERAL, principal, op, ALLOW)
}

/// DENY of `op` on the literal topic `name` for `principal`.
pub fn deny(name: &str, principal: &str, op: i8) -> AclCreation {
    topic_acl(name, LITERAL, principal, op, DENY)
}

/// ALLOW of `op` on every topic that starts with `prefix`.
pub fn allow_prefixed(prefix: &str, principal: &str, op: i8) -> AclCreation {
    topic_acl(prefix, PREFIXED, principal, op, ALLOW)
}

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

impl Rig {
    pub async fn create_topics_as(
        &self,
        who: &Caller,
        version: i16,
        topics: Vec<CreatableTopic>,
        validate_only: bool,
    ) -> Vec<CreatableTopicResult> {
        let mut req = CreateTopicsRequest::default();
        req.topics = topics;
        req.validate_only = validate_only;
        let Response::CreateTopics(r) = self.ask_as(who, version, Request::CreateTopics(req)).await
        else {
            panic!("not CreateTopics")
        };
        r.topics
    }

    /// DeleteTopics by name at `version`.
    pub async fn delete_topics_as(
        &self,
        who: &Caller,
        version: i16,
        names: &[&str],
    ) -> Vec<DeletableTopicResult> {
        let mut req = DeleteTopicsRequest::default();
        if version >= 6 {
            req.topics = names
                .iter()
                .map(|n| {
                    let mut s =
                        kafka_wire::messages::delete_topics_request::DeleteTopicState::default();
                    s.name = Some(wire_name(n));
                    s
                })
                .collect();
        } else {
            req.topic_names = names.iter().map(|n| wire_name(n)).collect();
        }
        let Response::DeleteTopics(r) = self.ask_as(who, version, Request::DeleteTopics(req)).await
        else {
            panic!("not DeleteTopics")
        };
        r.responses
    }

    /// DeleteTopics (v6) by id.
    pub async fn delete_topic_ids_as(
        &self,
        who: &Caller,
        ids: &[kafka_wire::Uuid],
    ) -> Vec<DeletableTopicResult> {
        let mut req = DeleteTopicsRequest::default();
        for id in ids {
            let mut s = kafka_wire::messages::delete_topics_request::DeleteTopicState::default();
            s.topic_id = *id;
            req.topics.push(s);
        }
        let Response::DeleteTopics(r) = self.ask_as(who, 6, Request::DeleteTopics(req)).await
        else {
            panic!("not DeleteTopics")
        };
        r.responses
    }

    /// The id of a topic, as the super user's Metadata says it.
    pub async fn id_of(&self, name: &str) -> kafka_wire::Uuid {
        let r = self
            .metadata_as(&crate::support::acls::admin(), 12, Some(&[name]), false)
            .await;
        r.topics[0].topic_id
    }

    /// Produce (v9) of one small batch to each `(topic, partition)`, in one request, as `who`.
    pub async fn produce_to_as(
        &self,
        who: &Caller,
        version: i16,
        acks: i16,
        targets: &[(&str, i32)],
    ) -> ProduceResponse {
        let mut req = produce_request(acks, targets[0].0, &[]);
        req.topic_data.clear();
        for (name, partition) in targets {
            let one = produce_request(acks, name, &[(*partition, records(1))]);
            req.topic_data.extend(one.topic_data);
        }
        self.produce_as(who, version, req).await
    }

    pub async fn produce_as(
        &self,
        who: &Caller,
        version: i16,
        req: ProduceRequest,
    ) -> ProduceResponse {
        let Response::Produce(r) = self.ask_as(who, version, Request::Produce(req)).await else {
            panic!("not Produce")
        };
        r
    }

    /// Every partition of a Produce answer: `(topic, partition, error)`.
    pub fn produced(r: &ProduceResponse) -> Vec<(String, i32, i16)> {
        r.responses
            .iter()
            .flat_map(|t| {
                t.partition_responses
                    .iter()
                    .map(|p: &PartitionProduceResponse| {
                        (t.name.0.to_string(), p.index, p.error_code)
                    })
            })
            .collect()
    }

    /// Fetch (v12) of the partitions, waiting for nothing, as `who`.
    pub async fn fetch_topics_as(
        &self,
        who: &Caller,
        version: i16,
        asks: &[crate::support::Ask<'_>],
    ) -> FetchResponse {
        let req = crate::support::fetch_request(asks, 0, 1, 1 << 20);
        let Response::Fetch(r) = self.ask_as(who, version, Request::Fetch(req)).await else {
            panic!("not Fetch")
        };
        r
    }

    /// Every partition of a Fetch answer: `(topic, partition, error)`.
    pub fn fetched(r: &FetchResponse) -> Vec<(String, i32, i16)> {
        r.responses
            .iter()
            .flat_map(|t| {
                t.partitions
                    .iter()
                    .map(|p| (t.topic.0.to_string(), p.partition_index, p.error_code))
            })
            .collect()
    }

    /// ListOffsets (latest) of the partitions of each topic, in one request.
    pub async fn list_offsets_as(
        &self,
        who: &Caller,
        version: i16,
        asks: &[(&str, &[i32])],
    ) -> ListOffsetsResponse {
        let mut req = ListOffsetsRequest::default();
        for (name, partitions) in asks {
            let mut t = ListOffsetsTopic::default();
            t.name = wire_name(name);
            for p in *partitions {
                let mut q = ListOffsetsPartition::default();
                q.partition_index = *p;
                q.timestamp = -1;
                q.current_leader_epoch = -1;
                t.partitions.push(q);
            }
            req.topics.push(t);
        }
        let Response::ListOffsets(r) = self.ask_as(who, version, Request::ListOffsets(req)).await
        else {
            panic!("not ListOffsets")
        };
        r
    }

    /// DeleteRecords up to the high watermark (-1) of the partitions of each topic.
    pub async fn delete_records_as(
        &self,
        who: &Caller,
        version: i16,
        asks: &[(&str, &[i32])],
    ) -> DeleteRecordsResponse {
        let mut req = DeleteRecordsRequest::default();
        for (name, partitions) in asks {
            let mut t = DeleteRecordsTopic::default();
            t.name = wire_name(name);
            for p in *partitions {
                let mut q = DeleteRecordsPartition::default();
                q.partition_index = *p;
                q.offset = -1;
                t.partitions.push(q);
            }
            req.topics.push(t);
        }
        let Response::DeleteRecords(r) =
            self.ask_as(who, version, Request::DeleteRecords(req)).await
        else {
            panic!("not DeleteRecords")
        };
        r
    }

    /// CreatePartitions as `who`: `(name, error, message)` of each result.
    pub async fn create_partitions_as(
        &self,
        who: &Caller,
        version: i16,
        topics: &[(&str, i32)],
    ) -> Vec<(String, i16, Option<String>)> {
        use kafka_wire::messages::CreatePartitionsRequest;
        use kafka_wire::messages::create_partitions_request::CreatePartitionsTopic;
        let mut req = CreatePartitionsRequest::default();
        req.timeout_ms = 10_000;
        for (name, count) in topics {
            let mut t = CreatePartitionsTopic::default();
            t.name = wire_name(name);
            t.count = *count;
            t.assignments = None;
            req.topics.push(t);
        }
        let Response::CreatePartitions(r) = self
            .ask_as(who, version, Request::CreatePartitions(req))
            .await
        else {
            panic!("not CreatePartitions")
        };
        r.results
            .into_iter()
            .map(|r| {
                (
                    r.name.0.to_string(),
                    r.error_code,
                    r.error_message.map(|m| m.to_string()),
                )
            })
            .collect()
    }

    /// DescribeConfigs of the resources `(type, name)`, as `who`.
    pub async fn describe_configs_as(
        &self,
        who: &Caller,
        version: i16,
        resources: &[(i8, &str)],
    ) -> Vec<DescribeConfigsResult> {
        let mut req = DescribeConfigsRequest::default();
        for (kind, name) in resources {
            let mut r = DescribeConfigsResource::default();
            r.resource_type = *kind;
            r.resource_name = text(name);
            r.configuration_keys = None;
            req.resources.push(r);
        }
        let Response::DescribeConfigs(r) = self
            .ask_as(who, version, Request::DescribeConfigs(req))
            .await
        else {
            panic!("not DescribeConfigs")
        };
        r.results
    }

    /// AlterConfigs (legacy, v2) of `(type, name)` setting `retention.ms` to 5000: `(code, message)` of each answer, in order.
    pub async fn alter_as(
        &self,
        who: &Caller,
        resources: &[(i8, &str)],
    ) -> Vec<(i16, Option<String>)> {
        use kafka_wire::messages::AlterConfigsRequest;
        use kafka_wire::messages::alter_configs_request::{AlterConfigsResource, AlterableConfig};
        let mut req = AlterConfigsRequest::default();
        for (kind, name) in resources {
            let mut r = AlterConfigsResource::default();
            r.resource_type = *kind;
            r.resource_name = text(name);
            let mut c = AlterableConfig::default();
            c.name = text(change_for(*kind).0);
            c.value = Some(text(change_for(*kind).1));
            r.configs.push(c);
            req.resources.push(r);
        }
        let Response::AlterConfigs(r) = self.ask_as(who, 2, Request::AlterConfigs(req)).await
        else {
            panic!("not AlterConfigs")
        };
        r.responses
            .into_iter()
            .map(|a| (a.error_code, a.error_message.map(|m| m.to_string())))
            .collect()
    }

    /// IncrementalAlterConfigs (v1) of `(type, name)` setting `retention.ms` to 5000, as `who`.
    pub async fn incremental_as(
        &self,
        who: &Caller,
        resources: &[(i8, &str)],
    ) -> Vec<(i16, Option<String>)> {
        use kafka_wire::messages::IncrementalAlterConfigsRequest;
        use kafka_wire::messages::incremental_alter_configs_request::{
            AlterConfigsResource, AlterableConfig,
        };
        let mut req = IncrementalAlterConfigsRequest::default();
        for (kind, name) in resources {
            let mut r = AlterConfigsResource::default();
            r.resource_type = *kind;
            r.resource_name = text(name);
            let mut c = AlterableConfig::default();
            c.name = text(change_for(*kind).0);
            c.config_operation = 0;
            c.value = Some(text(change_for(*kind).1));
            r.configs.push(c);
            req.resources.push(r);
        }
        let Response::IncrementalAlterConfigs(r) = self
            .ask_as(who, 1, Request::IncrementalAlterConfigs(req))
            .await
        else {
            panic!("not IncrementalAlterConfigs")
        };
        r.responses
            .into_iter()
            .map(|a| (a.error_code, a.error_message.map(|m| m.to_string())))
            .collect()
    }

    /// DescribeProducers (v0) of the partitions of each topic, as `who`.
    pub async fn describe_producers_as(
        &self,
        who: &Caller,
        asks: &[(&str, &[i32])],
    ) -> DescribeProducersResponse {
        let mut req = DescribeProducersRequest::default();
        for (name, partitions) in asks {
            let mut t = TopicRequest::default();
            t.name = wire_name(name);
            t.partition_indexes = partitions.to_vec();
            req.topics.push(t);
        }
        let Response::DescribeProducers(r) =
            self.ask_as(who, 0, Request::DescribeProducers(req)).await
        else {
            panic!("not DescribeProducers")
        };
        r
    }
}

/// A closed-world rig (default refuses, `User:admin` is super) holding the topics `names`, one partition each, with one
/// record in each.
pub async fn rig_with_topics(names: &[&str]) -> Rig {
    let rig = crate::support::rig_with(crate::support::acls::closed_world);
    let admin = crate::support::acls::admin();
    let made = rig
        .create_topics_as(
            &admin,
            7,
            names.iter().map(|n| topic(n, 1)).collect(),
            false,
        )
        .await;
    assert!(made.iter().all(|r| r.error_code == 0), "{made:?}");
    for n in names {
        let r = rig.produce_to_as(&admin, 9, -1, &[(n, 0)]).await;
        assert_eq!(Rig::produced(&r)[0].2, 0);
    }
    rig
}

/// Gives `acls` to the cluster as the super user.
pub async fn grant(rig: &Rig, acls: Vec<AclCreation>) {
    crate::support::acls::grant(rig, acls).await;
}

/// The change the AlterConfigs helpers make to a resource of `kind`: one that is valid for it (a topic's retention, or
/// the broker's retention bytes), so that a request that is refused is refused for its rights and nothing else.
fn change_for(kind: i8) -> (&'static str, &'static str) {
    if kind == 4 {
        ("log.retention.bytes", "9000")
    } else {
        ("retention.ms", "5000")
    }
}
