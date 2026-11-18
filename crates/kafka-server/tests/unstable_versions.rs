//! InitProducerId v6 on a real socket, as Kafka 4.4.0 serves it (docs/port/behaviours/transactions.md TX-27): with the
//! defaults ApiVersions lists 0 to 5 and a request of v6 closes the connection unanswered; with the unstable versions on
//! (`--unstable-api-versions`) 0 to 6 are listed and v6 is answered.
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
use kafka_wire::messages::{
    ApiKey, ApiVersionsRequest, ApiVersionsResponse, InitProducerIdRequest, InitProducerIdResponse,
    TransactionalId,
};
use kafka_wire::testing::{framed, request_payload};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

async fn start(unstable: bool) -> Running {
    let config = Config {
        listen: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        ..Config::default()
    };
    let mut broker = BrokerConfig::new("/data".into());
    broker.unstable_api_versions = unstable;
    let broker = Broker::open(SimFs::new(), broker).unwrap();
    Server::bind(config, Arc::new(broker)).await.unwrap()
}

/// The answer's body, or `None` if the connection was closed instead.
async fn answer(socket: &mut TcpStream) -> Option<Bytes> {
    let mut len = [0; 4];
    timeout(Duration::from_secs(5), socket.read_exact(&mut len))
        .await
        .unwrap()
        .ok()?;
    let mut payload = vec![0; u32::from_be_bytes(len) as usize];
    socket.read_exact(&mut payload).await.ok()?;
    Some(Bytes::from(payload))
}

async fn init_producer_id_range(socket: &mut TcpStream) -> (i16, i16) {
    let mut body = ApiVersionsRequest::default();
    body.client_software_name = StrBytes::from_static_str("test");
    body.client_software_version = StrBytes::from_static_str("1");
    socket
        .write_all(&framed(&request_payload(ApiKey::ApiVersions, 3, 1, None, &body)))
        .await
        .unwrap();
    let mut payload = answer(socket).await.unwrap();
    let _correlation_id = payload.split_to(4);
    let r = ApiVersionsResponse::decode(&mut payload, 3).unwrap();
    let k = r
        .api_keys
        .iter()
        .find(|k| k.api_key == ApiKey::InitProducerId as i16)
        .unwrap();
    (k.min_version, k.max_version)
}

async fn init_v6(socket: &mut TcpStream) -> Option<InitProducerIdResponse> {
    let mut body = InitProducerIdRequest::default();
    body.transactional_id = Some(TransactionalId(StrBytes::from_static_str("tx")));
    body.transaction_timeout_ms = 60_000;
    socket
        .write_all(&framed(&request_payload(ApiKey::InitProducerId, 6, 2, Some("test"), &body)))
        .await
        .unwrap();
    let mut payload = answer(socket).await?;
    let _correlation_id = payload.split_to(4);
    // Flexible response header: one byte of (no) tagged fields.
    let _tags = payload.split_to(1);
    Some(InitProducerIdResponse::decode(&mut payload, 6).unwrap())
}

#[tokio::test]
async fn init_producer_id_v6_is_listed_and_served_only_with_the_unstable_versions() {
    let plain = start(false).await;
    let mut socket = TcpStream::connect(plain.local_addr()).await.unwrap();
    assert_eq!(init_producer_id_range(&mut socket).await, (0, 5));
    assert!(init_v6(&mut socket).await.is_none(), "closed unanswered, as Kafka 4.4.0 does");

    let unstable = start(true).await;
    let mut socket = TcpStream::connect(unstable.local_addr()).await.unwrap();
    assert_eq!(init_producer_id_range(&mut socket).await, (0, 6));
    let r = init_v6(&mut socket).await.expect("answered");
    assert_eq!((r.error_code, r.producer_epoch), (0, 0));
    assert_eq!(r.ongoing_txn_producer_id.0, -1);
}
