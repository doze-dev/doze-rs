//! The eleven topic APIs as one enum, for the tables of authorization tests (slice 8d-1): the operation each needs on the
//! topic, two topic names to act on, and a call that answers the code of its one item.
#![allow(
    unreachable_pub,
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use crate::support::acls::{ALLOW, CLUSTER, CREATE, DESCRIBE, LITERAL, READ, WRITE, creation};
use crate::support::{self, Rig};
use crate::topics::*;
use kafka_broker::Caller;

pub const ALTER: i8 = 7;
pub const TOPIC_RESOURCE: i8 = 2;
pub const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;

#[derive(Clone, Copy, Debug)]
pub enum Api {
    CreateTopics,
    DeleteTopics,
    Produce,
    Fetch,
    ListOffsets,
    DeleteRecords,
    CreatePartitions,
    DescribeConfigs,
    AlterConfigs,
    IncrementalAlterConfigs,
    DescribeProducers,
}

pub const APIS: [Api; 11] = [
    Api::CreateTopics,
    Api::DeleteTopics,
    Api::Produce,
    Api::Fetch,
    Api::ListOffsets,
    Api::DeleteRecords,
    Api::CreatePartitions,
    Api::DescribeConfigs,
    Api::AlterConfigs,
    Api::IncrementalAlterConfigs,
    Api::DescribeProducers,
];

impl Api {
    /// The operation on the topic the API needs.
    pub fn op(self) -> i8 {
        match self {
            Api::CreateTopics => CREATE,
            Api::DeleteTopics | Api::DeleteRecords => DELETE,
            Api::Produce => WRITE,
            Api::Fetch | Api::DescribeProducers => READ,
            Api::ListOffsets => DESCRIBE,
            Api::CreatePartitions => ALTER,
            Api::DescribeConfigs => DESCRIBE_CONFIGS,
            Api::AlterConfigs | Api::IncrementalAlterConfigs => ALTER_CONFIGS,
        }
    }

    /// Two topic names for the API to act on: CreateTopics needs ones that are not there yet.
    pub fn names(self) -> (&'static str, &'static str) {
        match self {
            Api::CreateTopics => ("c", "f"),
            _ => ("t", "u"),
        }
    }

    /// An operation that does not imply `self.op()`.
    pub fn other(self) -> i8 {
        if self.op() == CREATE { READ } else { CREATE }
    }

    /// The code the API answers for its one item, asking about `topic`.
    pub async fn code(self, rig: &Rig, who: &Caller, topic: &str) -> i16 {
        match self {
            Api::CreateTopics => {
                let made = support::topic(topic, 1);
                rig.create_topics_as(who, 7, vec![made], false).await[0].error_code
            }
            Api::DeleteTopics => rig.delete_topics_as(who, 5, &[topic]).await[0].error_code,
            Api::Produce => Rig::produced(&rig.produce_to_as(who, 9, -1, &[(topic, 0)]).await)[0].2,
            Api::Fetch => {
                let r = rig
                    .fetch_topics_as(who, 12, &[(topic, 0, 0, 1 << 20)])
                    .await;
                Rig::fetched(&r)[0].2
            }
            Api::ListOffsets => {
                rig.list_offsets_as(who, 7, &[(topic, &[0])]).await.topics[0].partitions[0]
                    .error_code
            }
            Api::DeleteRecords => {
                rig.delete_records_as(who, 2, &[(topic, &[0])]).await.topics[0].partitions[0]
                    .error_code
            }
            Api::CreatePartitions => rig.create_partitions_as(who, 3, &[(topic, 2)]).await[0].1,
            Api::DescribeConfigs => {
                rig.describe_configs_as(who, 4, &[(TOPIC_RESOURCE, topic)])
                    .await[0]
                    .error_code
            }
            Api::AlterConfigs => rig.alter_as(who, &[(TOPIC_RESOURCE, topic)]).await[0].0,
            Api::IncrementalAlterConfigs => {
                rig.incremental_as(who, &[(TOPIC_RESOURCE, topic)]).await[0].0
            }
            Api::DescribeProducers => {
                rig.describe_producers_as(who, &[(topic, &[0])])
                    .await
                    .topics[0]
                    .partitions[0]
                    .error_code
            }
        }
    }
}

/// A closed-world rig holding topics `t` and `u`, one record in each.
pub async fn world() -> Rig {
    rig_with_topics(&["t", "u"]).await
}

/// CreateTopics and DeleteTopics ask the cluster first, and with the default allowing, a cluster no ACL covers is open to
/// everyone, which covers every topic (`hasClusterAuth`, `ControllerApis.createTopics` and `.deleteTopics`). An ACL for
/// somebody else on the cluster turns that off, so that the topic's own ACLs decide.
pub async fn cover_the_cluster(rig: &Rig, api: Api) {
    let op = match api {
        Api::CreateTopics => CREATE,
        Api::DeleteTopics => DELETE,
        _ => return,
    };
    let acl = creation(
        (CLUSTER, "kafka-cluster", LITERAL),
        "User:nobody",
        op,
        ALLOW,
    );
    grant(rig, vec![acl]).await;
}
