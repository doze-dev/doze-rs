//! ListOffsets.

use kafka_meta::Topic;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::ids::{PartitionId, TimestampMs, TopicName};
use kafka_wire::messages::list_offsets_request::ListOffsetsPartition;
use kafka_wire::messages::list_offsets_response::{
    ListOffsetsPartitionResponse, ListOffsetsTopicResponse,
};
use kafka_wire::messages::{ListOffsetsRequest, ListOffsetsResponse};

use crate::{Broker, blocking};

const LATEST: i64 = -1;
const EARLIEST: i64 = -2;
const MAX_TIMESTAMP: i64 = -3;

impl<F: Fs> Broker<F> {
    /// Answers earliest, latest and max-timestamp queries and, for a real timestamp, the offset of the first record at or
    /// after it ("replay from ten minutes ago"). A timestamp after every record is offset -1 with timestamp -1.
    pub(crate) async fn list_offsets(
        &self,
        version: i16,
        req: &ListOffsetsRequest,
    ) -> ListOffsetsResponse {
        let mut resp = ListOffsetsResponse::default();
        for t in &req.topics {
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
        // -4 and -5 came with v8 and v9, which are not offered; max timestamp (-3) came with v7.
        if p.timestamp < MAX_TIMESTAMP || (p.timestamp == MAX_TIMESTAMP && version < 7) {
            return refuse(r, ResponseError::UnsupportedVersion);
        }
        let partition = match self.partitions.get_unless_empty(topic, id).await {
            Ok(Some(partition)) => partition,
            // Nothing was ever written: it starts and ends at 0, and has no record at any time.
            Ok(None) => {
                if matches!(p.timestamp, LATEST | EARLIEST) {
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
        let reader = partition.reader();
        let found = match p.timestamp {
            // Under read-committed the last stable offset is what a consumer may read, and with no transactions
            // yet that is the end.
            LATEST => Ok(Some((reader.end_offset(), None))),
            EARLIEST => Ok(Some((reader.start_offset(), None))),
            MAX_TIMESTAMP => blocking::run(|| reader.offset_of_max_timestamp())
                .map(|f| f.map(|(o, t)| (o, Some(t)))),
            ts => blocking::run(|| reader.offset_for_timestamp(TimestampMs::from_wire(ts)))
                .map(|f| f.map(|(o, t)| (o, Some(t)))),
        };
        match found {
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
