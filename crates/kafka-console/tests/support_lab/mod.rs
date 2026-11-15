//! Shared by the Lab's live tests: a real broker whose server has the recorder and the fault table, a raw client whose calls may meet a
//! closed connection, and the requests the tests send.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    unused_imports
)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::support_log::records;
use crate::support_write::{World, world};
use bytes::Bytes;
use kafka_auth::Principal;
use kafka_broker::{Caller, LabOutcome};
use kafka_console::Recorder;
use kafka_console::gather::lab as glab;
use kafka_console::lab::Lab;
use kafka_console::model::lab::{LabFields, LabKind, LabMade, LabOnceDone};
use kafka_console::model::live::LabDid;
use kafka_protocol::protocol::{Decodable, Encodable};
use kafka_server::fault::FaultTable;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::join_group_request::JoinGroupRequestProtocol;
use kafka_wire::messages::*;
use kafka_wire::testing::{framed, request_payload};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub struct Lw {
    pub w: World,
    pub lab: Lab,
    pub rec: Arc<Recorder>,
}

pub async fn lab_world(name: &str) -> Lw {
    let table = FaultTable::new();
    let rec = Recorder::new();
    let (t, r) = (Arc::clone(&table), Arc::clone(&rec));
    let w = world(name, move |bc, sc| {
        sc.observer = Some(r);
        sc.fault = Some(t);
        bc.acl_default_allow = true;
    })
    .await;
    w.create_topic("orders", 1, &[]).await;
    Lw {
        w,
        lab: Lab::new(table),
        rec,
    }
}

/// A raw client whose calls may meet a closed connection.
pub struct Raw {
    pub stream: TcpStream,
    pub id: i32,
    pub client: &'static str,
}

impl Raw {
    pub async fn connect(w: &World, client: &'static str) -> Self {
        let addr = w.server.as_ref().unwrap().local_addr();
        Self {
            stream: TcpStream::connect(addr).await.unwrap(),
            id: 0,
            client,
        }
    }

    pub async fn try_call<R: Encodable>(
        &mut self,
        key: ApiKey,
        version: i16,
        body: &R,
    ) -> Option<Bytes> {
        self.id += 1;
        let req = framed(&request_payload(
            key,
            version,
            self.id,
            Some(self.client),
            body,
        ));
        let _ = self.stream.write_all(&req).await;
        let mut len = [0; 4];
        tokio::time::timeout(Duration::from_secs(5), self.stream.read_exact(&mut len))
            .await
            .ok()?
            .ok()?;
        let mut payload = vec![0; u32::from_be_bytes(len) as usize];
        self.stream.read_exact(&mut payload).await.ok()?;
        Some(Bytes::from(payload))
    }
}

pub fn body<T: Decodable>(key: ApiKey, version: i16, mut payload: Bytes) -> T {
    let _ = kafka_protocol::messages::ResponseHeader::decode(
        &mut payload,
        key.response_header_version(version),
    )
    .unwrap();
    T::decode(&mut payload, version).unwrap()
}

pub fn s(t: &str) -> StrBytes {
    StrBytes::from_string(t.to_owned())
}

pub fn produce_req(topic: &str) -> ProduceRequest {
    let mut p = kafka_wire::messages::produce_request::PartitionProduceData::default();
    p.index = 0;
    p.records = Some(records(2));
    let mut t = kafka_wire::messages::produce_request::TopicProduceData::default();
    t.name = TopicName(s(topic));
    t.partition_data.push(p);
    let mut r = ProduceRequest::default();
    r.acks = -1;
    r.timeout_ms = 5000;
    r.topic_data.push(t);
    r
}

pub fn fetch_req(topic: &str) -> FetchRequest {
    let mut p = kafka_wire::messages::fetch_request::FetchPartition::default();
    p.partition = 0;
    p.partition_max_bytes = 1 << 20;
    let mut t = kafka_wire::messages::fetch_request::FetchTopic::default();
    t.topic = TopicName(s(topic));
    t.partitions.push(p);
    let mut r = FetchRequest::default();
    r.max_wait_ms = 0;
    r.max_bytes = 1 << 20;
    r.topics.push(t);
    r
}

pub fn fields(pairs: &[(&str, &str)]) -> LabFields {
    let f: Vec<(String, String)> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    kafka_console::lab::form::fields_of(&f)
}

pub async fn make(l: &Lw, kind: LabKind, pairs: &[(&str, &str)]) -> u32 {
    match glab::create(&l.lab, &l.w.backend, Some(&l.rec), kind, fields(pairs)).await {
        LabMade::Made(id) => id,
        LabMade::Invalid(v) => panic!("{v:?}"),
    }
}

pub fn wire_hits(l: &Lw, rule: u32) -> Vec<kafka_console::model::WireRow> {
    l.rec.lab_hits(rule, 50).0
}

pub fn join(group: &str, member: &str) -> JoinGroupRequest {
    let mut r = JoinGroupRequest::default();
    r.group_id = GroupId(s(group));
    r.member_id = s(member);
    r.session_timeout_ms = 30_000;
    r.rebalance_timeout_ms = 30_000;
    r.protocol_type = s("consumer");
    let mut p = JoinGroupRequestProtocol::default();
    p.name = s("range");
    r.protocols = vec![p];
    r
}

pub async fn classic_member(w: &World, group: &str) -> String {
    let Response::JoinGroup(j) = w.ask(3, Request::JoinGroup(join(group, ""))).await else {
        panic!()
    };
    assert_eq!(j.error_code, 0, "{j:?}");
    let mut sync = SyncGroupRequest::default();
    sync.group_id = GroupId(s(group));
    sync.generation_id = j.generation_id;
    sync.member_id = j.member_id.clone();
    let Response::SyncGroup(a) = w.ask(1, Request::SyncGroup(sync)).await else {
        panic!()
    };
    assert_eq!(a.error_code, 0);
    j.member_id.to_string()
}

pub async fn heartbeat(w: &World, group: &str, member: &str, generation: i32) -> i16 {
    let mut h = HeartbeatRequest::default();
    h.group_id = GroupId(s(group));
    h.member_id = s(member);
    h.generation_id = generation;
    let Response::Heartbeat(r) = w.ask(1, Request::Heartbeat(h)).await else {
        panic!()
    };
    r.error_code
}
