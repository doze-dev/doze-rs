//! The server on a real socket: it answers, it limits, and it stops.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use kafka_broker::{Broker, Config as BrokerConfig};
use kafka_protocol::protocol::{Decodable, StrBytes};
use kafka_server::{Config, Running, Server};
use kafka_storage::sim::SimFs;
use kafka_wire::batch::build::BatchBuilder;
use kafka_wire::messages::fetch_request::{FetchPartition, FetchTopic};
use kafka_wire::messages::metadata_request::MetadataRequestTopic;
use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_wire::messages::{
    ApiKey, ApiVersionsRequest, ApiVersionsResponse, FetchRequest, MetadataRequest, ProduceRequest,
    TopicName,
};
use kafka_wire::testing::{framed, request_payload};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

async fn start(max_connections: usize) -> Running {
    let config = Config {
        listen: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        max_connections,
        ..Config::default()
    };
    let broker = Broker::open(SimFs::new(), BrokerConfig::new("/data".into())).unwrap();
    Server::bind(config, Arc::new(broker)).await.unwrap()
}

/// Asks ApiVersions and returns the keys listed, or `None` if the connection was closed instead.
async fn ask(socket: &mut TcpStream) -> Option<Vec<i16>> {
    let mut body = ApiVersionsRequest::default();
    body.client_software_name = StrBytes::from_static_str("test");
    body.client_software_version = StrBytes::from_static_str("1");
    let request = framed(&request_payload(ApiKey::ApiVersions, 3, 1, None, &body));
    let _ = socket.write_all(&request).await;
    let mut len = [0; 4];
    socket.read_exact(&mut len).await.ok()?;
    let mut payload = vec![0; u32::from_be_bytes(len) as usize];
    socket.read_exact(&mut payload).await.ok()?;
    let mut payload = Bytes::from(payload);
    let _correlation_id = payload.split_to(4);
    let answer = ApiVersionsResponse::decode(&mut payload, 3).unwrap();
    Some(answer.api_keys.iter().map(|k| k.api_key).collect())
}

#[tokio::test]
async fn a_client_on_a_socket_gets_its_answer() {
    let server = start(10).await;
    let mut socket = TcpStream::connect(server.local_addr()).await.unwrap();
    let served = [
        ApiKey::Produce,
        ApiKey::Fetch,
        ApiKey::ListOffsets,
        ApiKey::Metadata,
        ApiKey::FindCoordinator,
        ApiKey::JoinGroup,
        ApiKey::Heartbeat,
        ApiKey::LeaveGroup,
        ApiKey::SyncGroup,
        ApiKey::ApiVersions,
        ApiKey::CreateTopics,
        ApiKey::DeleteTopics,
        ApiKey::InitProducerId,
    ];
    assert_eq!(
        ask(&mut socket).await,
        Some(served.map(|k| k as i16).to_vec())
    );
    server.shutdown().await;
}

#[tokio::test]
async fn connections_past_the_limit_are_closed_and_a_freed_place_is_reused() {
    let server = start(1).await;
    let mut first = TcpStream::connect(server.local_addr()).await.unwrap();
    assert!(ask(&mut first).await.is_some());
    let mut second = TcpStream::connect(server.local_addr()).await.unwrap();
    assert_eq!(
        ask(&mut second).await,
        None,
        "the second connection is refused"
    );
    drop(first);
    // The place comes back when the first connection's task has noticed the hang-up.
    let mut third = loop {
        let mut socket = TcpStream::connect(server.local_addr()).await.unwrap();
        if ask(&mut socket).await.is_some() {
            break socket;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert!(ask(&mut third).await.is_some());
    server.shutdown().await;
}

#[tokio::test]
async fn shutdown_closes_idle_connections_and_returns() {
    let server = start(10).await;
    let addr = server.local_addr();
    let mut idle = TcpStream::connect(addr).await.unwrap();
    assert!(ask(&mut idle).await.is_some());
    timeout(Duration::from_secs(5), server.shutdown())
        .await
        .expect("shutdown does not wait for a connection that has nothing to say");
    let mut rest = Vec::new();
    assert_eq!(idle.read_to_end(&mut rest).await.unwrap(), 0);
    assert!(
        TcpStream::connect(addr).await.is_err(),
        "nothing is listening any more"
    );
}

/// Sends one request and returns the answer's payload after its correlation id.
async fn exchange<R: kafka_protocol::protocol::Encodable>(
    socket: &mut TcpStream,
    key: ApiKey,
    version: i16,
    correlation_id: i32,
    body: &R,
) -> Bytes {
    send(socket, key, version, correlation_id, body).await;
    let mut len = [0; 4];
    socket.read_exact(&mut len).await.unwrap();
    let mut payload = vec![0; u32::from_be_bytes(len) as usize];
    socket.read_exact(&mut payload).await.unwrap();
    Bytes::from(payload).split_off(4)
}

async fn send<R: kafka_protocol::protocol::Encodable>(
    socket: &mut TcpStream,
    key: ApiKey,
    version: i16,
    correlation_id: i32,
    body: &R,
) {
    let request = framed(&request_payload(key, version, correlation_id, None, body));
    socket.write_all(&request).await.unwrap();
}

/// Makes topic "t" (one partition) by asking about it.
async fn make_topic(socket: &mut TcpStream) {
    let mut topic = MetadataRequestTopic::default();
    topic.name = Some(TopicName(StrBytes::from_static_str("t")));
    let mut req = MetadataRequest::default();
    req.topics = Some(vec![topic]);
    req.allow_auto_topic_creation = true;
    exchange(socket, ApiKey::Metadata, 12, 7, &req).await;
}

#[tokio::test]
async fn stopping_the_server_does_not_wait_out_a_fetch_that_is_waiting_for_records() {
    let server = start(10).await;
    let mut socket = TcpStream::connect(server.local_addr()).await.unwrap();
    make_topic(&mut socket).await;
    let mut partition = FetchPartition::default();
    partition.partition = 0;
    partition.fetch_offset = 0;
    partition.partition_max_bytes = 1 << 20;
    let mut topic = FetchTopic::default();
    topic.topic = TopicName(StrBytes::from_static_str("t"));
    topic.partitions.push(partition);
    let mut req = FetchRequest::default();
    req.max_wait_ms = 60_000;
    req.min_bytes = 1;
    req.max_bytes = 1 << 20;
    req.topics.push(topic);
    send(&mut socket, ApiKey::Fetch, 12, 8, &req).await;
    tokio::time::sleep(Duration::from_millis(200)).await; // the Fetch is now waiting for a record
    let stopped = std::time::Instant::now();
    timeout(Duration::from_secs(10), server.shutdown())
        .await
        .expect("a stop is not held up by a long poll");
    assert!(
        stopped.elapsed() < Duration::from_secs(5),
        "the Fetch had a minute to wait: {:?}",
        stopped.elapsed()
    );
}

#[tokio::test]
async fn an_acks_zero_produce_gets_no_answer_and_the_connection_goes_on_in_order() {
    let server = start(10).await;
    let mut socket = TcpStream::connect(server.local_addr()).await.unwrap();
    make_topic(&mut socket).await;
    let mut partition = PartitionProduceData::default();
    partition.index = 0;
    partition.records = Some(BatchBuilder::new().records(1).build());
    let mut topic = TopicProduceData::default();
    topic.name = TopicName(StrBytes::from_static_str("t"));
    topic.partition_data.push(partition);
    let mut req = ProduceRequest::default();
    req.acks = 0;
    req.timeout_ms = 5000;
    req.topic_data.push(topic);
    send(&mut socket, ApiKey::Produce, 9, 9, &req).await;
    // The first thing on the wire is the answer to the next request (correlation id 1), not to the Produce (9).
    send(
        &mut socket,
        ApiKey::ApiVersions,
        3,
        1,
        &ApiVersionsRequest::default(),
    )
    .await;
    let mut len = [0; 4];
    socket.read_exact(&mut len).await.unwrap();
    let mut payload = vec![0; u32::from_be_bytes(len) as usize];
    socket.read_exact(&mut payload).await.unwrap();
    assert_eq!(
        payload[..4],
        1_i32.to_be_bytes(),
        "answered out of order, or a Produce was answered"
    );
    server.shutdown().await;
}
