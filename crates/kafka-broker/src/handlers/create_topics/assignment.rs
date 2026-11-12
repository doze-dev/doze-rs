//! How many partitions a CreateTopics entry asks for: a count, or a manual assignment that gives one.

use std::collections::{HashMap, HashSet};

use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::ids::PartitionCount;
use kafka_wire::messages::create_topics_request::CreatableTopic;

use super::Refusal;
use crate::Broker;

impl<F: Fs> Broker<F> {
    /// How many partitions the request asks for: its manual assignment's length, or its count (-1 is the broker's
    /// default).
    pub(super) fn partitions_requested(
        &self,
        t: &CreatableTopic,
    ) -> Result<PartitionCount, Refusal> {
        let invalid = |m: &str| (ResponseError::InvalidRequest, m.to_owned());
        if !t.assignments.is_empty() {
            if t.replication_factor != -1 {
                return Err(invalid(
                    "A manual partition assignment was specified, but replication factor was not set to -1.",
                ));
            }
            if t.num_partitions != -1 {
                return Err(invalid(
                    "A manual partition assignment was specified, but numPartitions was not set to -1.",
                ));
            }
            return self.assigned_partitions(t);
        }
        if t.replication_factor == 0 || t.replication_factor < -1 {
            let why = "Replication factor must be larger than 0, or -1 to use the default value.";
            return Err((ResponseError::InvalidReplicationFactor, why.to_owned()));
        }
        // Greater than one is accepted and answered as one: a development broker has one node, and clients
        // (Connect, Streams) ask for three by default.
        let count = if t.num_partitions == -1 {
            self.config.default_partitions
        } else {
            t.num_partitions
        };
        PartitionCount::new(count).ok_or_else(|| {
            let why = "Number of partitions was set to an invalid non-positive value.";
            (ResponseError::InvalidPartitions, why.to_owned())
        })
    }

    fn assigned_partitions(&self, t: &CreatableTopic) -> Result<PartitionCount, Refusal> {
        let bad = |m: String| (ResponseError::InvalidReplicaAssignment, m);
        let mut replicas = HashMap::new();
        for a in &t.assignments {
            if replicas.insert(a.partition_index, &a.broker_ids).is_some() {
                return Err(bad(format!(
                    "Partition {} is assigned more than once.",
                    a.partition_index
                )));
            }
        }
        let count = i32::try_from(t.assignments.len()).unwrap_or(i32::MAX);
        for p in 0..count {
            let Some(brokers) = replicas.get(&p) else {
                return Err(bad(
                    "partitions should be a consecutive 0-based integer sequence".to_owned(),
                ));
            };
            if brokers.is_empty() {
                return Err(bad(format!(
                    "The manual partition assignment includes an empty replica list for partition {p}."
                )));
            }
            let mut seen = HashSet::new();
            for b in brokers.iter() {
                if b.0 != self.config.node_id {
                    return Err(bad(format!(
                        "The manual partition assignment includes broker {}, but no such broker is registered.",
                        b.0
                    )));
                }
                if !seen.insert(b.0) {
                    return Err(bad(format!(
                        "The manual partition assignment includes the broker {} more than once.",
                        b.0
                    )));
                }
            }
        }
        PartitionCount::new(count).ok_or_else(|| {
            bad("partitions should be a consecutive 0-based integer sequence".to_owned())
        })
    }
}
