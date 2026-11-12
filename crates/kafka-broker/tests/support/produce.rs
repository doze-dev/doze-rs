//! Requests and readbacks for Produce.
#![allow(unreachable_pub, dead_code, clippy::unwrap_used, clippy::panic)]

use super::{DATA, Rig, at, head, wire_name};
use kafka_storage::sim::SimFs;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::ApiKey;

use bytes::Bytes;
use kafka_storage::{LogConfig, PartitionLog, Trust};
use kafka_wire::batch::build::BatchBuilder;
use kafka_wire::ids::Offset;
use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_wire::messages::produce_response::PartitionProduceResponse;
use kafka_wire::messages::{
    InitProducerIdRequest, InitProducerIdResponse, ProduceRequest, ProduceResponse,
};

/// A valid batch of `n` records.
pub fn records(n: u32) -> Bytes {
    BatchBuilder::new().records(n).build()
}

pub fn produce_request(acks: i16, topic: &str, partitions: &[(i32, Bytes)]) -> ProduceRequest {
    let mut data = TopicProduceData::default();
    data.name = wire_name(topic);
    for (index, bytes) in partitions {
        let mut p = PartitionProduceData::default();
        p.index = *index;
        p.records = Some(bytes.clone());
        data.partition_data.push(p);
    }
    let mut req = ProduceRequest::default();
    req.acks = acks;
    req.timeout_ms = 5000;
    req.topic_data.push(data);
    req
}

impl Rig {
    /// What the broker does with a Produce at `version`.
    pub async fn produce_outcome(
        &self,
        version: i16,
        req: ProduceRequest,
    ) -> kafka_broker::Outcome {
        let head = head(ApiKey::Produce, version);
        self.broker
            .handle(&at(), &super::caller(), &head, Request::Produce(req))
            .await
    }

    /// A Produce at v9 to one partition, answered.
    pub async fn produce(
        &self,
        acks: i16,
        topic: &str,
        partition: i32,
        batch: Bytes,
    ) -> PartitionProduceResponse {
        let req = produce_request(acks, topic, &[(partition, batch)]);
        let Response::Produce(r) = self.ask(9, Request::Produce(req)).await else {
            panic!("not Produce")
        };
        let ProduceResponse { mut responses, .. } = r;
        responses.remove(0).partition_responses.remove(0)
    }

    pub async fn init_producer_id(&self, transactional_id: Option<&str>) -> InitProducerIdResponse {
        let mut req = InitProducerIdRequest::default();
        req.transactional_id = transactional_id
            .map(|t| kafka_wire::messages::TransactionalId(StrBytes::from_string(t.to_owned())));
        req.transaction_timeout_ms = -1;
        let Response::InitProducerId(r) = self.ask(4, Request::InitProducerId(req)).await else {
            panic!("not InitProducerId")
        };
        r
    }
}

/// The batches a partition holds on `fs`, read back by opening its log as a restarted broker would.
pub fn stored(fs: &SimFs, topic: &str, partition: i32) -> Vec<Bytes> {
    let dir = std::path::Path::new(DATA).join(format!("{topic}-{partition}"));
    let log = PartitionLog::open(fs.clone(), dir, LogConfig::default(), Trust::Unvouched).unwrap();
    let reader = log.reader();
    let mut out = Vec::new();
    let mut at = reader.start_offset();
    while at < reader.end_offset() {
        let got = reader.read(at, usize::MAX >> 1, None).unwrap();
        let peek = kafka_wire::batch::Peek::read(&got).unwrap();
        let whole = got.slice(..peek.total_len as usize);
        at = Offset::new(peek.base_offset + i64::from(peek.record_count)).unwrap();
        out.push(whole);
    }
    out
}

/// A batch of records with these keys and values (`None`: no value, a tombstone), timestamped at 1,000.
pub fn keyed(pairs: &[(&str, Option<&str>)]) -> Bytes {
    use kafka_wire::batch::build::Rec;
    let recs: Vec<Rec> = pairs
        .iter()
        .map(|(k, v)| Rec {
            key: Some(k.as_bytes().to_vec()),
            value: v.map(|v| v.as_bytes().to_vec()),
            ..Rec::default()
        })
        .collect();
    BatchBuilder::from_records(1_000, &recs).build()
}
