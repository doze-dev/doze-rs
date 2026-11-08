//! Requests for Fetch and ListOffsets.
#![allow(unreachable_pub, dead_code, clippy::unwrap_used, clippy::panic)]

use super::{Rig, wire_name};
use bytes::Bytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::fetch_request::{FetchPartition, FetchTopic};
use kafka_wire::messages::fetch_response::PartitionData;
use kafka_wire::messages::list_offsets_request::{ListOffsetsPartition, ListOffsetsTopic};
use kafka_wire::messages::list_offsets_response::ListOffsetsPartitionResponse;
use kafka_wire::messages::{FetchRequest, FetchResponse, ListOffsetsRequest};

/// What a Fetch asks of one partition: (topic, partition, offset, the partition's byte limit).
pub type Ask<'a> = (&'a str, i32, i64, i32);

pub fn fetch_request(
    asks: &[Ask<'_>],
    max_wait_ms: i32,
    min_bytes: i32,
    max_bytes: i32,
) -> FetchRequest {
    let mut req = FetchRequest::default();
    req.max_wait_ms = max_wait_ms;
    req.min_bytes = min_bytes;
    req.max_bytes = max_bytes;
    for (name, partition, offset, limit) in asks {
        let mut p = FetchPartition::default();
        p.partition = *partition;
        p.fetch_offset = *offset;
        p.partition_max_bytes = *limit;
        match req.topics.iter_mut().find(|t| &*t.topic.0 == *name) {
            Some(t) => t.partitions.push(p),
            None => {
                let mut t = FetchTopic::default();
                t.topic = wire_name(name);
                t.partitions.push(p);
                req.topics.push(t);
            }
        }
    }
    req
}

impl Rig {
    /// A Fetch at v12 that waits for nothing.
    pub async fn fetch_now(&self, asks: &[Ask<'_>]) -> Vec<PartitionData> {
        self.fetch(fetch_request(asks, 0, 1, 1 << 20)).await
    }

    pub async fn fetch(&self, req: FetchRequest) -> Vec<PartitionData> {
        let Response::Fetch(FetchResponse { responses, .. }) =
            self.ask(12, Request::Fetch(req)).await
        else {
            panic!("not Fetch")
        };
        responses.into_iter().flat_map(|t| t.partitions).collect()
    }

    /// ListOffsets at `version` for one partition and one timestamp.
    pub async fn list_offset(
        &self,
        version: i16,
        topic: &str,
        partition: i32,
        timestamp: i64,
    ) -> ListOffsetsPartitionResponse {
        self.list_offsets(version, topic, 0, partition_query(partition, timestamp, -1))
            .await
    }

    /// ListOffsets at v7 with a given isolation level and leader epoch.
    pub async fn list_offset_with(
        &self,
        topic: &str,
        timestamp: i64,
        isolation: i8,
        epoch: i32,
    ) -> ListOffsetsPartitionResponse {
        self.list_offsets(7, topic, isolation, partition_query(0, timestamp, epoch))
            .await
    }

    async fn list_offsets(
        &self,
        version: i16,
        topic: &str,
        isolation: i8,
        p: ListOffsetsPartition,
    ) -> ListOffsetsPartitionResponse {
        let mut t = ListOffsetsTopic::default();
        t.name = wire_name(topic);
        t.partitions.push(p);
        let mut req = ListOffsetsRequest::default();
        req.isolation_level = isolation;
        req.topics.push(t);
        let Response::ListOffsets(r) = self.ask(version, Request::ListOffsets(req)).await else {
            panic!("not ListOffsets")
        };
        r.topics
            .into_iter()
            .next()
            .unwrap()
            .partitions
            .into_iter()
            .next()
            .unwrap()
    }
}

fn partition_query(partition: i32, timestamp: i64, epoch: i32) -> ListOffsetsPartition {
    let mut p = ListOffsetsPartition::default();
    p.partition_index = partition;
    p.timestamp = timestamp;
    p.current_leader_epoch = epoch;
    p
}

/// The offsets at which the batches in `records` start.
pub fn batch_bases(records: &Bytes) -> Vec<i64> {
    kafka_wire::batch::Batches::new(records.clone())
        .map(|b| b.unwrap().base_offset().unwrap().get())
        .collect()
}
