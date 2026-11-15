//! SEC-292: a client's telemetry payload is its own words and the broker never reads them, so no byte of one is in any log, at any
//! level, whether the push was taken or refused.
//!
//! A test binary of its own, with one test, so that the logging it installs is the process's and no other test's callsites compete.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use kafka_broker::{Broker, Config as BrokerConfig};
use kafka_protocol::protocol::{Decodable, Encodable};
use kafka_server::{Config, Server};
use kafka_storage::sim::SimFs;
use kafka_wire::messages::{
    ApiKey, GetTelemetrySubscriptionsRequest, GetTelemetrySubscriptionsResponse,
    PushTelemetryRequest, PushTelemetryResponse,
};
use kafka_wire::testing::{framed, request_payload};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const PLANTED: &str = "planted-telemetry-secret-7731";

#[derive(Clone, Default)]
struct Logged(Arc<Mutex<Vec<u8>>>);

impl io::Write for Logged {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// One request and its answer's body (the flexible response header is a correlation id and no tagged fields).
async fn exchange<R: Encodable>(socket: &mut TcpStream, key: ApiKey, id: i32, body: &R) -> Bytes {
    let request = framed(&request_payload(key, 0, id, Some("planted-client"), body));
    socket.write_all(&request).await.unwrap();
    let mut len = [0; 4];
    socket.read_exact(&mut len).await.unwrap();
    let mut payload = vec![0; u32::from_be_bytes(len) as usize];
    socket.read_exact(&mut payload).await.unwrap();
    Bytes::from(payload).split_off(5)
}

async fn push(socket: &mut TcpStream, id: i32, req: &PushTelemetryRequest) -> i16 {
    let mut answer = exchange(socket, ApiKey::PushTelemetry, id, req).await;
    PushTelemetryResponse::decode(&mut answer, 0)
        .unwrap()
        .error_code
}

#[tokio::test]
async fn no_payload_is_logged_at_any_level_taken_or_refused() {
    let logged = Logged::default();
    let sink = logged.clone();
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || sink.clone())
        .init();
    let config = Config {
        listen: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        ..Config::default()
    };
    let broker = Broker::open(SimFs::new(), BrokerConfig::new("/data".into())).unwrap();
    let server = Server::bind(config, Arc::new(broker)).await.unwrap();
    let mut socket = TcpStream::connect(server.local_addr()).await.unwrap();

    let mut answers = Vec::new();
    for n in 0..5 {
        let mut ask = exchange(
            &mut socket,
            ApiKey::GetTelemetrySubscriptions,
            n,
            &GetTelemetrySubscriptionsRequest::default(),
        )
        .await;
        let s = GetTelemetrySubscriptionsResponse::decode(&mut ask, 0).unwrap();
        let mut req = PushTelemetryRequest::default();
        req.client_instance_id = s.client_instance_id;
        req.subscription_id = s.subscription_id;
        req.metrics = Bytes::from(PLANTED.repeat(3));
        // Taken; refused as corrupt under a codec; for a subscription it was not given; over the limit; terminating.
        match n {
            1 => req.compression_type = 1,
            2 => req.subscription_id += 1,
            3 => req.metrics = Bytes::from(PLANTED.repeat(36_200)),
            4 => req.terminating = true,
            _ => {}
        }
        answers.push(push(&mut socket, 100 + n, &req).await);
    }
    assert_eq!(
        answers,
        [0, 87, 117, 118, 0],
        "the pushes did what was meant"
    );
    drop(socket);
    tokio::time::sleep(Duration::from_millis(300)).await;
    server.shutdown().await;

    let text = String::from_utf8_lossy(&logged.0.lock().unwrap()).into_owned();
    assert!(
        text.contains("connection closed"),
        "the log was captured: {text:?}"
    );
    assert!(!text.contains("planted-telemetry"), "{text}");
    assert!(!text.contains("telemetry-secret"), "{text}");
    assert!(!text.contains("7731"), "{text}");
}
