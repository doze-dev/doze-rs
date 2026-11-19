//! ElectLeaders (API 43, KIP-460): elect a new leader for some partitions, or all. What `kafka-leader-election.sh` and the
//! admin clients' `electLeaders` send.
//!
//! On one node every partition has its one replica as its leader, which is also its preferred leader, so no election is ever
//! needed: Kafka 4.4.0's controller (`ReplicationControlManager.electLeaders`, `electLeader`) answers each partition named
//! ELECTION_NOT_NEEDED, and leaves every partition out of an answer to a request for all of them, which lists each topic with
//! no partitions. Nothing changes, so the epochs stay where they are.

use kafka_auth::authz::Operation;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::ids::TopicName;
use kafka_wire::messages::elect_leaders_response::{PartitionResult, ReplicaElectionResult};
use kafka_wire::messages::{ElectLeadersRequest, ElectLeadersResponse, TopicName as WireName};

use crate::{Broker, Caller};

/// The election types (`ElectionType`): PREFERRED 0, UNCLEAN 1.
const ELECTION_TYPES: [i8; 2] = [0, 1];

/// `Errors.ELECTION_NOT_NEEDED`.
const NOT_NEEDED: &str = "Leader election not needed for topic partition.";

impl<F: Fs> Broker<F> {
    /// Answers each partition named with what electing its leader would do; for a null list, every topic with nothing to
    /// report.
    ///
    /// The caller needs ALTER on the cluster (`ControllerApis.handleElectLeaders`); without it, and for an election type
    /// Kafka does not know (`InvalidRequestException`, from v1, which has the field), the whole request is refused in the
    /// shape of `ElectLeadersRequest.getErrorResponse`: the error at the top (from v1) and on every partition named, with its
    /// message. An empty list is answered empty (`QuorumController.electLeaders`).
    pub(crate) fn elect_leaders(
        &self,
        caller: &Caller,
        req: &ElectLeadersRequest,
        version: i16,
    ) -> ElectLeadersResponse {
        if !self.cluster_allows(caller, Operation::Alter) {
            let why = caller.cluster_denied("ALTER");
            return refused(
                req,
                version,
                ResponseError::ClusterAuthorizationFailed,
                &why,
            );
        }
        if req.topic_partitions.as_ref().is_some_and(Vec::is_empty) {
            return ElectLeadersResponse::default();
        }
        if !ELECTION_TYPES.contains(&req.election_type) {
            let why = format!("Unknown election type {}", req.election_type);
            return refused(req, version, ResponseError::InvalidRequest, &why);
        }
        let mut resp = ElectLeadersResponse::default();
        let Some(asked) = &req.topic_partitions else {
            resp.replica_election_results = (self.meta.topics().iter())
                .map(|t| result(&t.name.to_string(), Vec::new()))
                .collect();
            return resp;
        };
        for topic in asked {
            let name = topic.topic.0.as_str();
            let found = TopicName::parse(name)
                .ok()
                .and_then(|n| self.meta.topic(&n));
            let partitions = (topic.partitions.iter())
                .map(|&p| match &found {
                    None => line(
                        p,
                        ResponseError::UnknownTopicOrPartition,
                        Some(format!("No such topic as {name}")),
                    ),
                    Some(t) if p < 0 || p >= t.partitions.get() => line(
                        p,
                        ResponseError::UnknownTopicOrPartition,
                        Some(format!("No such partition as {name}-{p}")),
                    ),
                    Some(_) => line(
                        p,
                        ResponseError::ElectionNotNeeded,
                        Some(NOT_NEEDED.to_owned()),
                    ),
                })
                .collect();
            resp.replica_election_results.push(result(name, partitions));
        }
        resp
    }
}

fn result(topic: &str, partitions: Vec<PartitionResult>) -> ReplicaElectionResult {
    let mut r = ReplicaElectionResult::default();
    r.topic = WireName(StrBytes::from_string(topic.to_owned()));
    r.partition_result = partitions;
    r
}

fn line(partition: i32, error: ResponseError, message: Option<String>) -> PartitionResult {
    let mut p = PartitionResult::default();
    p.partition_id = partition;
    p.error_code = error.code();
    p.error_message = message.map(StrBytes::from_string);
    p
}

/// `ElectLeadersRequest.getErrorResponse`: the error at the top (v0 has no field for it), and on every partition named with
/// the message.
fn refused(
    req: &ElectLeadersRequest,
    version: i16,
    error: ResponseError,
    why: &str,
) -> ElectLeadersResponse {
    let mut resp = ElectLeadersResponse::default();
    if version >= 1 {
        resp.error_code = error.code();
    }
    resp.replica_election_results = (req.topic_partitions.iter().flatten())
        .map(|t| {
            let partitions = (t.partitions.iter())
                .map(|&p| line(p, error, Some(why.to_owned())))
                .collect();
            result(t.topic.0.as_str(), partitions)
        })
        .collect();
    resp
}
