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
                if version == 0 {
                    out.partitions
                        .push(self.list_old(topic.as_deref(), p).await);
                    continue;
                }
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

impl<F: Fs> Broker<F> {
    /// ListOffsets v0: up to `max_num_offsets` offsets, newest first, for the time asked, as Kafka answered before KIP-79
    /// (`Log.legacyFetchOffsetsBefore`). The answer is not about records but about segments: each segment's first offset
    /// (no lower than the log start) stands with the time its file was last written, and the log end stands with now
    /// (if the last segment holds anything). LATEST counts back from the end, EARLIEST is the first alone, and a time
    /// counts back from the last of those whose time is at or before it (none: no offsets). A partition never written is
    /// one empty segment as old as now.
    async fn list_old(&self, topic: Option<&Topic>, p: &ListOffsetsPartition) -> ListOffsetsPartitionResponse {
        let mut r = ListOffsetsPartitionResponse::default();
        r.partition_index = p.partition_index;
        let Some((topic, id)) = topic
            .zip(PartitionId::new(p.partition_index))
            .filter(|(t, id)| id.get() < t.partitions.get())
        else {
            r.error_code = ResponseError::UnknownTopicOrPartition.code();
            return r;
        };
        let now = crate::clock::now().get();
        let table = match self.partitions.get_unless_empty(topic, id).await {
            Ok(Some(partition)) => segment_times(partition.reader(), now),
            Ok(None) => vec![(0, now)],
            Err(e) => {
                tracing::error!(error = %e, topic = %topic.name, partition = p.partition_index, "list offsets could not open a partition");
                r.error_code = ResponseError::KafkaStorageError.code();
                return r;
            }
        };
        r.old_style_offsets = legacy_offsets(&table, p.timestamp, p.max_num_offsets);
        r
    }
}

/// Each segment's first offset (no lower than the log start) and when its file was last written, then the end and now if
/// the last segment holds anything.
fn segment_times<F: Fs>(reader: &kafka_storage::LogReader<F>, now: i64) -> Vec<(i64, i64)> {
    let start = reader.start_offset().get();
    let segments = reader.segments();
    let mut table: Vec<(i64, i64)> = segments
        .iter()
        .map(|s| (s.base.get().max(start), s.modified_ms.unwrap_or(now)))
        .collect();
    if segments.last().is_none_or(|s| s.bytes > 0) {
        table.push((reader.end_offset().get(), now));
    }
    table
}

/// `Log.legacyFetchOffsetsBefore` over `table` (offsets and times, oldest first).
fn legacy_offsets(table: &[(i64, i64)], timestamp: i64, max: i32) -> Vec<i64> {
    let last = table.len().checked_sub(1);
    let from = match timestamp {
        LATEST => last,
        EARLIEST => (!table.is_empty()).then_some(0),
        t => table.iter().rposition(|(_, time)| *time <= t),
    };
    let Some(from) = from else {
        return Vec::new();
    };
    let take = usize::try_from(max).unwrap_or(0).min(from + 1);
    let mut out: Vec<i64> = table
        .get(..=from)
        .unwrap_or_default()
        .iter()
        .rev()
        .take(take)
        .map(|(offset, _)| *offset)
        .collect();
    out.sort_unstable_by(|a, b| b.cmp(a));
    out
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
        // (v0's answer has neither field; their defaults, -1, are not written.)
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
