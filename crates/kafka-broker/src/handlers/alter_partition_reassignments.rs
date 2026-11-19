//! AlterPartitionReassignments (API 45, KIP-455) and ListPartitionReassignments (API 46): move a partition's replicas, and
//! list the moves in progress. What `kafka-reassign-partitions.sh` sends.
//!
//! On one node the only replica set a partition can have is this broker, which it has: Kafka 4.4.0's controller
//! (`ReplicationControlManager.alterPartitionReassignments`, `changePartitionReassignment`) accepts a reassignment to the
//! replicas a partition already has and finishes it at once, with no record, so a reassignment is never in progress, a
//! cancellation is always NO_REASSIGNMENT_IN_PROGRESS, and the list is always empty. A target naming another broker, no broker,
//! or one broker twice is refused as the controller refuses it (`validateManualPartitionAssignment`).

use kafka_auth::authz::Operation;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::ids::TopicName;
use kafka_wire::messages::alter_partition_reassignments_request::ReassignablePartition;
use kafka_wire::messages::alter_partition_reassignments_response::{
    ReassignablePartitionResponse, ReassignableTopicResponse,
};
use kafka_wire::messages::{
    AlterPartitionReassignmentsRequest, AlterPartitionReassignmentsResponse,
    ListPartitionReassignmentsRequest, ListPartitionReassignmentsResponse,
};

use crate::{Broker, Caller};

/// Why one partition's reassignment was refused, as `ApiError.fromThrowable` reports it: the error, and the exception's
/// message where it is not the error's own text (then Kafka sends no message).
type Refusal = (ResponseError, Option<String>);

impl<F: Fs> Broker<F> {
    /// Answers each partition named: no error for a target that is the replicas it has, else the controller's refusal. The
    /// caller needs ALTER on the cluster (`ControllerApis.handleAlterPartitionReassignments`); without it every partition
    /// named, and the top, is CLUSTER_AUTHORIZATION_FAILED with the request's text
    /// (`AlterPartitionReassignmentsRequest.getErrorResponse`). An empty request is answered with the schema's defaults
    /// (`QuorumController.alterPartitionReassignments`).
    pub(crate) fn alter_partition_reassignments(
        &self,
        caller: &Caller,
        req: &AlterPartitionReassignmentsRequest,
    ) -> AlterPartitionReassignmentsResponse {
        let mut resp = AlterPartitionReassignmentsResponse::default();
        if !self.cluster_allows(caller, Operation::Alter) {
            let why = caller.cluster_denied("ALTER");
            let code = ResponseError::ClusterAuthorizationFailed.code();
            resp.error_code = code;
            resp.error_message = Some(StrBytes::from_string(why.clone()));
            resp.responses = (req.topics.iter())
                .map(|t| {
                    let partitions = (t.partitions.iter())
                        .map(|p| line(p.partition_index, code, Some(why.clone())))
                        .collect();
                    topic(t.name.0.as_str(), partitions)
                })
                .collect();
            return resp;
        }
        if req.topics.is_empty() {
            return resp;
        }
        resp.error_message = None;
        resp.allow_replication_factor_change = req.allow_replication_factor_change;
        for t in &req.topics {
            let name = t.name.0.as_str();
            let partitions = (t.partitions.iter())
                .map(
                    |p| match self.reassign(name, p, req.allow_replication_factor_change) {
                        Ok(()) => line(p.partition_index, 0, None),
                        Err((error, message)) => line(p.partition_index, error.code(), message),
                    },
                )
                .collect();
            resp.responses.push(topic(name, partitions));
        }
        resp
    }

    /// `alterPartitionReassignment`: the topic, the partition, then the target (or the cancellation).
    fn reassign(
        &self,
        name: &str,
        p: &ReassignablePartition,
        allow_rf_change: bool,
    ) -> Result<(), Refusal> {
        let unknown = |m: String| (ResponseError::UnknownTopicOrPartition, Some(m));
        let topic = TopicName::parse(name)
            .ok()
            .and_then(|n| self.meta.topic(&n))
            .ok_or_else(|| unknown(format!("Unable to find a topic named {name}.")))?;
        let index = p.partition_index;
        if index < 0 || index >= topic.partitions.get() {
            return Err(unknown(format!("Unable to find partition {name}:{index}.")));
        }
        let Some(target) = &p.replicas else {
            // Nothing is ever in progress; the exception's text is the error's own, so no message is sent.
            return Err((ResponseError::NoReassignmentInProgress, None));
        };
        let invalid = |m: String| (ResponseError::InvalidReplicaAssignment, Some(m));
        if target.is_empty() {
            return Err(invalid(
                "The manual partition assignment includes an empty replica list.".to_owned(),
            ));
        }
        let mut sorted: Vec<i32> = target.iter().map(|b| b.0).collect();
        sorted.sort_unstable();
        let mut previous = None;
        for broker in sorted.iter().copied() {
            if broker != self.config.node_id {
                return Err(invalid(format!(
                    "The manual partition assignment includes broker {broker}, but no such broker is registered."
                )));
            }
            if previous == Some(broker) {
                return Err(invalid(format!(
                    "The manual partition assignment includes the broker {broker} more than once."
                )));
            }
            previous = Some(broker);
        }
        // Only reached with one replica, this broker: the factor is the one there is.
        if !allow_rf_change && sorted.len() != 1 {
            return Err((
                ResponseError::InvalidReplicationFactor,
                Some(format!(
                    "The replication factor is changed from 1 to {}",
                    sorted.len()
                )),
            ));
        }
        Ok(())
    }

    /// The reassignments in progress, which on one node are none. The caller needs DESCRIBE on the cluster
    /// (`ControllerApis.handleListPartitionReassignments`); without it the topics named are echoed back with their
    /// partitions and the error (`ListPartitionReassignmentsRequest.getErrorResponse`).
    pub(crate) fn list_partition_reassignments(
        &self,
        caller: &Caller,
        req: &ListPartitionReassignmentsRequest,
    ) -> ListPartitionReassignmentsResponse {
        let mut resp = ListPartitionReassignmentsResponse::default();
        if !self.cluster_allows(caller, Operation::Describe) {
            use kafka_wire::messages::list_partition_reassignments_response::{
                OngoingPartitionReassignment, OngoingTopicReassignment,
            };
            resp.error_code = ResponseError::ClusterAuthorizationFailed.code();
            resp.error_message = Some(StrBytes::from_string(caller.cluster_denied("DESCRIBE")));
            resp.topics = (req.topics.iter().flatten())
                .map(|t| {
                    let mut o = OngoingTopicReassignment::default();
                    o.name = t.name.clone();
                    o.partitions = (t.partition_indexes.iter())
                        .map(|&i| {
                            let mut p = OngoingPartitionReassignment::default();
                            p.partition_index = i;
                            p
                        })
                        .collect();
                    o
                })
                .collect();
            return resp;
        }
        resp.error_message = None;
        resp
    }
}

fn topic(name: &str, partitions: Vec<ReassignablePartitionResponse>) -> ReassignableTopicResponse {
    let mut t = ReassignableTopicResponse::default();
    t.name = kafka_wire::messages::TopicName(StrBytes::from_string(name.to_owned()));
    t.partitions = partitions;
    t
}

fn line(partition: i32, code: i16, message: Option<String>) -> ReassignablePartitionResponse {
    let mut p = ReassignablePartitionResponse::default();
    p.partition_index = partition;
    p.error_code = code;
    p.error_message = message.map(StrBytes::from_string);
    p
}
