//! AlterReplicaLogDirs (API 34, KIP-113): move a partition's log to another of the broker's log directories. What
//! `kafka-reassign-partitions.sh` sends for a reassignment that names log directories, and the admin clients'
//! `alterReplicaLogDirs`.
//!
//! The broker has one log directory, its data directory (what DescribeLogDirs reports), and every partition's log is already
//! there, so the only move there is is to where the log is, which Kafka 4.4.0 accepts and does nothing for
//! (`Partition.maybeCreateFutureReplica` is false when the directory is the log's own). The checks are
//! `ReplicaManager.alterReplicaLogDirs`'s, in its order: a topic name too long for a future log's directory
//! (INVALID_TOPIC_EXCEPTION), a directory that is not one of the broker's (LOG_DIR_NOT_FOUND, compared as the absolute path
//! exactly), a partition the broker does not have (UNKNOWN_TOPIC_OR_PARTITION).

use std::collections::BTreeMap;

use kafka_auth::authz::Operation;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::ids::TopicName;
use kafka_wire::messages::alter_replica_log_dirs_response::{
    AlterReplicaLogDirPartitionResult, AlterReplicaLogDirTopicResult,
};
use kafka_wire::messages::{
    AlterReplicaLogDirsRequest, AlterReplicaLogDirsResponse, TopicName as WireName,
};

use crate::{Broker, Caller};

/// The longest name a directory may have: a future log's is `<topic>-<partition>.<32 hex digits>-future`
/// (`LocalLog.logFutureDirName`), refused above this.
const LONGEST_DIR_NAME: usize = 255;

impl<F: Fs> Broker<F> {
    /// Answers each partition named, once (a partition named for two directories takes the last), grouped by topic. The
    /// caller needs ALTER on the cluster; without it every partition is CLUSTER_AUTHORIZATION_FAILED, one entry per topic of
    /// each directory, as sent (`AlterReplicaLogDirsRequest.getErrorResponse`).
    pub(crate) fn alter_replica_log_dirs(
        &self,
        caller: &Caller,
        req: &AlterReplicaLogDirsRequest,
    ) -> AlterReplicaLogDirsResponse {
        let mut resp = AlterReplicaLogDirsResponse::default();
        if !self.cluster_allows(caller, Operation::Alter) {
            let code = ResponseError::ClusterAuthorizationFailed.code();
            resp.results = (req.dirs.iter())
                .flat_map(|d| &d.topics)
                .map(|t| result(&t.name, t.partitions.iter().map(|&p| (p, code)).collect()))
                .collect();
            return resp;
        }
        let ours = self.log_dir();
        // The last directory named for a partition wins (Kafka builds a map of them).
        let mut wanted: BTreeMap<(&str, i32), &str> = BTreeMap::new();
        let mut order: Vec<(&WireName, i32)> = Vec::new();
        for dir in &req.dirs {
            for t in &dir.topics {
                for &p in &t.partitions {
                    if wanted
                        .insert((t.name.0.as_str(), p), dir.path.as_str())
                        .is_none()
                    {
                        order.push((&t.name, p));
                    }
                }
            }
        }
        let mut by_topic: Vec<(&WireName, Vec<(i32, i16)>)> = Vec::new();
        for (name, p) in order {
            let dir = wanted
                .get(&(name.0.as_str(), p))
                .copied()
                .unwrap_or_default();
            let code = self
                .move_to(name.0.as_str(), p, dir, &ours)
                .map_or(0, |e| e.code());
            match by_topic.iter_mut().find(|(n, _)| *n == name) {
                Some((_, parts)) => parts.push((p, code)),
                None => by_topic.push((name, vec![(p, code)])),
            }
        }
        resp.results = (by_topic.into_iter())
            .map(|(name, parts)| result(name, parts))
            .collect();
        resp
    }

    /// The one log directory, as DescribeLogDirs reports it: the data directory's absolute path.
    pub(crate) fn log_dir(&self) -> String {
        let data = &self.config.data_dir;
        std::path::absolute(data)
            .unwrap_or_else(|_| data.clone())
            .display()
            .to_string()
    }

    /// What moving the partition's log to `dir` comes to: nothing to do, or the error.
    fn move_to(&self, topic: &str, partition: i32, dir: &str, ours: &str) -> Option<ResponseError> {
        // `<topic>-<partition>` then `.` and 32 hex digits then `-future`, counted in UTF-16 units as Java counts.
        let future_name =
            topic.encode_utf16().count() + 1 + partition.to_string().len() + 1 + 32 + 7;
        if future_name > LONGEST_DIR_NAME {
            return Some(ResponseError::InvalidTopicException);
        }
        if dir != ours {
            return Some(ResponseError::LogDirNotFound);
        }
        let found = TopicName::parse(topic)
            .ok()
            .and_then(|n| self.meta.topic(&n));
        match found {
            Some(t) if (0..t.partitions.get()).contains(&partition) => None,
            _ => Some(ResponseError::UnknownTopicOrPartition),
        }
    }
}

fn result(name: &WireName, partitions: Vec<(i32, i16)>) -> AlterReplicaLogDirTopicResult {
    let mut r = AlterReplicaLogDirTopicResult::default();
    r.topic_name = name.clone();
    r.partitions = (partitions.into_iter())
        .map(|(p, code)| {
            let mut out = AlterReplicaLogDirPartitionResult::default();
            out.partition_index = p;
            out.error_code = code;
            out
        })
        .collect();
    r
}
