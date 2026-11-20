//! ListOffsets.

use kafka_auth::authz::Operation;
use kafka_meta::Topic;
use kafka_storage::StorageError;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::ids::{Offset, PartitionId, TimestampMs, TopicName};
use kafka_wire::messages::list_offsets_request::{ListOffsetsPartition, ListOffsetsTopic};
use kafka_wire::messages::list_offsets_response::{
    ListOffsetsPartitionResponse, ListOffsetsTopicResponse,
};
use kafka_wire::messages::{ListOffsetsRequest, ListOffsetsResponse};

use crate::partitions::Partition;
use crate::{Broker, Caller, blocking};

const LATEST: i64 = -1;
const EARLIEST: i64 = -2;
const MAX_TIMESTAMP: i64 = -3;
/// The local log's start (KIP-405, v8): with no tiered storage, the log's start, as `EARLIEST`.
const EARLIEST_LOCAL: i64 = -4;
/// The newest offset in tiered storage (KIP-1005, v9): with none, offset -1 and leader epoch -1.
const LATEST_TIERED: i64 = -5;
/// The first offset not yet uploaded to tiered storage (KIP-1023, v11): with none, offset -1 and leader epoch -1.
const EARLIEST_PENDING_UPLOAD: i64 = -6;

/// The first version each special time may be asked at (`ReplicaManager.timestampMinSupportedVersion`); any other time
/// below 0 is asked at none.
fn first_version(timestamp: i64) -> Option<i16> {
    match timestamp {
        LATEST | EARLIEST => Some(0),
        MAX_TIMESTAMP => Some(7),
        EARLIEST_LOCAL => Some(8),
        LATEST_TIERED => Some(9),
        EARLIEST_PENDING_UPLOAD => Some(11),
        _ => None,
    }
}

impl<F: Fs> Broker<F> {
    /// Answers earliest, latest and max-timestamp queries and, for a real timestamp, the offset of the first record at or
    /// after it ("replay from ten minutes ago"). A timestamp after every record is offset -1 with timestamp -1. The times
    /// tiered storage added (v8, v9, v11) are answered as Kafka answers them for a topic with no tiered storage: the local
    /// start is the start, and the tiered ones are offset -1 at leader epoch -1. A special time below the version that
    /// brought it is UNSUPPORTED_VERSION, before the topic is looked for (`ReplicaManager.fetchOffset`).
    ///
    /// The caller must be allowed to DESCRIBE each topic (which any ACL for READ, WRITE, DELETE or ALTER grants); the
    /// check is made before the topic is looked for, and the topics refused are answered after those that were not
    /// (`KafkaApis.handleListOffsetRequest`).
    pub(crate) async fn list_offsets(
        &self,
        version: i16,
        caller: &Caller,
        req: &ListOffsetsRequest,
    ) -> ListOffsetsResponse {
        let mut resp = ListOffsetsResponse::default();
        let mut refused = Vec::new();
        for t in &req.topics {
            if !self.topic_allows(caller, &t.name.0, Operation::Describe) {
                refused.push(not_described(t));
                continue;
            }
            let topic = TopicName::parse(&t.name.0)
                .ok()
                .and_then(|n| self.meta.topic(&n));
            let mut out = ListOffsetsTopicResponse::default();
            out.name = t.name.clone();
            // (Apache Kafka answers a partition named twice in one topic once, with INVALID_REQUEST. This broker
            // answers each entry, because the console asks for a partition's earliest and latest offset as two
            // entries of one request.)
            for p in &t.partitions {
                let mut answer = self
                    .list_one(version, req.isolation_level, topic.as_deref(), p)
                    .await;
                // The leader epoch is a field from v4; a version without it must carry the default, or the
                // response cannot be written and the client's connection is closed.
                if version < 4 {
                    answer.leader_epoch = -1;
                }
                out.partitions.push(answer);
            }
            resp.topics.push(out);
        }
        resp.topics.extend(refused);
        resp
    }

    async fn list_one(
        &self,
        version: i16,
        isolation: i8,
        topic: Option<&Topic>,
        p: &ListOffsetsPartition,
    ) -> ListOffsetsPartitionResponse {
        let mut r = ListOffsetsPartitionResponse::default();
        r.partition_index = p.partition_index;
        r.timestamp = -1;
        r.offset = -1;
        r.leader_epoch = -1;
        let refuse = |mut r: ListOffsetsPartitionResponse, code: ResponseError| {
            r.error_code = code.code();
            r
        };
        if p.timestamp < 0 && first_version(p.timestamp).is_none_or(|first| version < first) {
            return refuse(r, ResponseError::UnsupportedVersion);
        }
        let Some((topic, id)) = topic.zip(PartitionId::new(p.partition_index)) else {
            return refuse(r, ResponseError::UnknownTopicOrPartition);
        };
        if id.get() >= topic.partitions.get() {
            return refuse(r, ResponseError::UnknownTopicOrPartition);
        }
        // Read-uncommitted (0) or read-committed (1); Kafka's answer to anything else is an unexplained error.
        if isolation != 0 && isolation != 1 {
            return refuse(r, ResponseError::UnknownServerError);
        }
        // A leader epoch from the future is one this broker has not had; 0 is the only one a single node has.
        if p.current_leader_epoch > 0 {
            return refuse(r, ResponseError::UnknownLeaderEpoch);
        }
        let partition = match self.partitions.get_unless_empty(topic, id).await {
            Ok(Some(partition)) => partition,
            // Nothing was ever written: it starts and ends at 0, and has no record at any time.
            Ok(None) => {
                if matches!(p.timestamp, LATEST | EARLIEST | EARLIEST_LOCAL) {
                    r.offset = 0;
                    r.leader_epoch = 0;
                }
                return r;
            }
            Err(e) => {
                tracing::error!(error = %e, topic = %topic.name, partition = p.partition_index, "list offsets could not open a partition");
                return refuse(r, ResponseError::KafkaStorageError);
            }
        };
        match lookup(&partition, p.timestamp, isolation) {
            Ok(Some((offset, time))) => {
                r.offset = offset.get();
                r.timestamp = time.map_or(-1, TimestampMs::get);
                // An answer that found an offset carries the leader epoch it was found under.
                r.leader_epoch = 0;
            }
            Ok(None) => {}
            Err(e) => {
                tracing::error!(error = %e, topic = %topic.name, partition = p.partition_index, "a timestamp lookup failed");
                return refuse(r, ResponseError::KafkaStorageError);
            }
        }
        r
    }
}

/// A topic the caller may not describe: each partition asked is 29, with no timestamp or offset.
fn not_described(t: &ListOffsetsTopic) -> ListOffsetsTopicResponse {
    let mut out = ListOffsetsTopicResponse::default();
    out.name = t.name.clone();
    for p in &t.partitions {
        let mut r = ListOffsetsPartitionResponse::default();
        r.partition_index = p.partition_index;
        r.error_code = ResponseError::TopicAuthorizationFailed.code();
        (r.timestamp, r.offset, r.leader_epoch) = (-1, -1, -1);
        out.partitions.push(r);
    }
    out
}

/// What the partition has for the time asked (`LATEST`, `EARLIEST`, the newest record, or a time): the offset, and the
/// time of the record if there is one.
fn lookup<F: Fs>(
    partition: &Partition<F>,
    timestamp: i64,
    isolation: i8,
) -> Result<Option<(Offset, Option<TimestampMs>)>, StorageError> {
    let reader = partition.reader();
    let found = match timestamp {
        // Under read-committed the last stable offset is what a consumer may read, and with no transactions yet that is
        // the end.
        LATEST if isolation == 1 => Ok(Some((partition.stability().last_stable, None))),
        LATEST => Ok(Some((reader.end_offset(), None))),
        EARLIEST | EARLIEST_LOCAL => Ok(Some((reader.start_offset(), None))),
        // Nothing is tiered: the offset -1, with leader epoch -1, which is the answer's default.
        LATEST_TIERED | EARLIEST_PENDING_UPLOAD => Ok(None),
        MAX_TIMESTAMP => {
            blocking::run(|| reader.offset_of_max_timestamp()).map(|f| f.map(|(o, t)| (o, Some(t))))
        }
        ts => blocking::run(|| reader.offset_for_timestamp(TimestampMs::from_wire(ts)))
            .map(|f| f.map(|(o, t)| (o, Some(t)))),
    };
    // Under read-committed a lookup by time (or for the newest record) that lands at or past the last stable offset has
    // found nothing a consumer may read: Kafka answers that as not found.
    match found {
        Ok(Some((offset, _)))
            if isolation == 1
                && !matches!(timestamp, LATEST | EARLIEST | EARLIEST_LOCAL)
                && offset.get() >= partition.stability().last_stable.get() =>
        {
            Ok(None)
        }
        other => other,
    }
}
