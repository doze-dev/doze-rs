//! DescribeLogDirs (API 35): the broker's log directory, the size of each partition in it, and (v4 on) the size of the
//! volume it is on and the room left there. `kafka-log-dirs.sh --describe` and the admin clients' `describeLogDirs`.
//!
//! The sizes are the data directory walk the Data page reads (`kafka_storage::usage::walk_data_dir`), so the two never
//! disagree; a partition's size is its segments' log files, as Kafka counts it (`UnifiedLog.size`).

use std::collections::{BTreeMap, BTreeSet};

use kafka_auth::authz::Operation;
use kafka_storage::fs::Fs;
use kafka_storage::usage::walk_data_dir;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::messages::describe_log_dirs_response::{
    DescribeLogDirsPartition, DescribeLogDirsResult, DescribeLogDirsTopic,
};
use kafka_wire::messages::{DescribeLogDirsRequest, DescribeLogDirsResponse, TopicName};

use crate::{Broker, Caller, blocking};

impl<F: Fs> Broker<F> {
    /// The one log directory, with every partition of every topic (or those asked for) and its size. A partition that has
    /// never been written to has no files yet and is 0 bytes, as a new partition's empty segment is in Kafka. The lag is
    /// always 0 and no log is a future one: there is one replica, and no log is being moved between directories.
    ///
    /// The caller needs DESCRIBE on the cluster; without it the answer is CLUSTER_AUTHORIZATION_FAILED and no directory
    /// (from v3, which has the field; before, just no directory: `KafkaApis.handleDescribeLogDirsRequest`). A partition asked
    /// for that does not exist is left out, and so is a topic with none left (`ReplicaManager.describeLogDirs`).
    pub(crate) fn describe_log_dirs(
        &self,
        caller: &Caller,
        req: &DescribeLogDirsRequest,
    ) -> DescribeLogDirsResponse {
        let mut resp = DescribeLogDirsResponse::default();
        if !self.cluster_allows(caller, Operation::Describe) {
            resp.error_code = ResponseError::ClusterAuthorizationFailed.code();
            return resp;
        }
        let data = &self.config.data_dir;
        let mut dir = DescribeLogDirsResult::default();
        dir.log_dir = StrBytes::from_string(
            std::path::absolute(data)
                .unwrap_or_else(|_| data.clone())
                .display()
                .to_string(),
        );
        let walked = match blocking::run(|| walk_data_dir(&self.fs, data)) {
            Ok(walked) => walked,
            Err(e) => {
                tracing::warn!(error = %e, "the data directory could not be described");
                dir.error_code = ResponseError::KafkaStorageError.code();
                resp.results.push(dir);
                return resp;
            }
        };
        if let Ok(Some(space)) = self.fs.space(data) {
            dir.total_bytes = clamp(space.total);
            dir.usable_bytes = clamp(space.free);
        }
        let sizes: BTreeMap<(&str, i32), u64> = (walked.partitions.iter())
            .map(|p| ((p.topic.as_str(), p.partition), p.log_bytes))
            .collect();
        let asked: Option<BTreeSet<(&str, i32)>> = req.topics.as_ref().map(|topics| {
            (topics.iter())
                .flat_map(|t| t.partitions.iter().map(|p| (t.topic.0.as_str(), *p)))
                .collect()
        });
        for topic in self.meta.topics() {
            let name = topic.name.as_str();
            let partitions: Vec<DescribeLogDirsPartition> = (0..topic.partitions.get())
                .filter(|p| asked.as_ref().is_none_or(|a| a.contains(&(name, *p))))
                .map(|p| {
                    let mut out = DescribeLogDirsPartition::default();
                    out.partition_index = p;
                    out.partition_size = clamp(sizes.get(&(name, p)).copied().unwrap_or(0));
                    out
                })
                .collect();
            if !partitions.is_empty() {
                let mut t = DescribeLogDirsTopic::default();
                t.name = TopicName(StrBytes::from_string(name.to_owned()));
                t.partitions = partitions;
                dir.topics.push(t);
            }
        }
        resp.results.push(dir);
        resp
    }
}

/// A byte count as the protocol's signed 64 bits (Java's `adjustForLargeFileSystems` makes an overflow `Long.MAX_VALUE`).
fn clamp(bytes: u64) -> i64 {
    i64::try_from(bytes).unwrap_or(i64::MAX)
}
