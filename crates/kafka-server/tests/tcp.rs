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
use kafka_wire::messages::{ApiKey, ApiVersionsRequest, ApiVersionsResponse};
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
        ApiKey::Metadata,
        ApiKey::ApiVersions,
        ApiKey::CreateTopics,
        ApiKey::DeleteTopics,
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
