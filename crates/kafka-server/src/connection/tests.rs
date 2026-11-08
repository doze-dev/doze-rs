use std::time::Duration;

use bytes::Bytes;
use kafka_protocol::protocol::{Decodable, StrBytes};
use kafka_wire::WireError;
use kafka_wire::messages::{ApiKey, ApiVersionsRequest, ApiVersionsResponse, MetadataRequest};
use kafka_wire::testing::{framed, request_payload};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, duplex};
use tokio::time::sleep;

use super::*;

fn context(timeouts: Timeouts) -> Context {
    let broker = Arc::new(Broker::new());
    Context {
        profile: Profile::new(kafka_wire::profile::KafkaMajor::DEFAULT),
        features: Features {
            served: broker.served(),
            ..Features::default()
        },
        broker,
        timeouts,
    }
}

fn api_versions(correlation_id: i32) -> Bytes {
    let mut body = ApiVersionsRequest::default();
    body.client_software_name = StrBytes::from_static_str("test");
    body.client_software_version = StrBytes::from_static_str("1");
    framed(&request_payload(
        ApiKey::ApiVersions,
        3,
        correlation_id,
        Some("t"),
        &body,
    ))
}

/// Reads one response to an ApiVersions v3 request: its correlation id and the keys it lists.
async fn read_answer(client: &mut DuplexStream) -> (i32, i16, Vec<i16>) {
    let mut len = [0; 4];
    client.read_exact(&mut len).await.unwrap();
    let mut payload = vec![0; u32::from_be_bytes(len) as usize];
    client.read_exact(&mut payload).await.unwrap();
    let mut payload = Bytes::from(payload);
    let id = i32::from_be_bytes(payload.split_to(4)[..].try_into().unwrap());
    let answer = ApiVersionsResponse::decode(&mut payload, 3).unwrap();
    let keys = answer.api_keys.iter().map(|k| k.api_key).collect();
    (id, answer.error_code, keys)
}

/// Starts serving one end of a pipe; the other end is the client.
fn start(
    timeouts: Timeouts,
    pipe: usize,
) -> (
    DuplexStream,
    tokio::task::JoinHandle<End>,
    CancellationToken,
) {
    let (client, server) = duplex(pipe);
    let stop = CancellationToken::new();
    let ctx = context(timeouts);
    let task = tokio::spawn({
        let stop = stop.clone();
        async move { serve(server, &ctx, &stop).await }
    });
    (client, task, stop)
}

#[tokio::test]
async fn api_versions_lists_only_what_the_broker_serves() {
    let (mut client, _task, _stop) = start(Timeouts::default(), 4096);
    client.write_all(&api_versions(7)).await.unwrap();
    let (id, error, keys) = read_answer(&mut client).await;
    assert_eq!((id, error), (7, 0));
    assert_eq!(keys, [ApiKey::ApiVersions as i16]);
}

#[tokio::test]
async fn requests_are_answered_in_order_whether_they_arrive_together_or_a_byte_at_a_time() {
    let (mut client, _task, _stop) = start(Timeouts::default(), 4096);
    let both = [api_versions(1), api_versions(2)].concat();
    client.write_all(&both).await.unwrap();
    for want in [1, 2] {
        assert_eq!(read_answer(&mut client).await.0, want);
    }
    for byte in api_versions(3).iter() {
        client.write_all(&[*byte]).await.unwrap();
    }
    assert_eq!(read_answer(&mut client).await.0, 3);
}

#[tokio::test]
async fn an_api_the_broker_does_not_serve_closes_the_connection_without_an_answer() {
    let (mut client, task, _stop) = start(Timeouts::default(), 4096);
    let metadata = request_payload(ApiKey::Metadata, 12, 1, None, &MetadataRequest::default());
    client.write_all(&framed(&metadata)).await.unwrap();
    assert!(matches!(
        task.await.unwrap(),
        End::Wire(WireError::NotAdvertised {
            key: ApiKey::Metadata,
            ..
        })
    ));
    let mut rest = Vec::new();
    client.read_to_end(&mut rest).await.unwrap();
    assert!(
        rest.is_empty(),
        "nothing is written to a client that asked for what is not served"
    );
}

#[tokio::test]
async fn a_frame_over_its_cap_closes_the_connection_before_the_body_is_waited_for() {
    let (mut client, task, _stop) = start(Timeouts::default(), 4096);
    client
        .write_all(&(2_000_000_000u32).to_be_bytes())
        .await
        .unwrap();
    assert!(matches!(
        task.await.unwrap(),
        End::Wire(WireError::FrameTooLarge { .. })
    ));
}

#[tokio::test(start_paused = true)]
async fn a_connection_with_nothing_to_say_is_closed_after_the_idle_time() {
    let timeouts = Timeouts {
        idle: Duration::from_secs(60),
        ..Timeouts::default()
    };
    let (mut client, task, _stop) = start(timeouts, 4096);
    client.write_all(&api_versions(1)).await.unwrap();
    read_answer(&mut client).await;
    sleep(Duration::from_secs(59)).await;
    assert!(!task.is_finished());
    assert!(matches!(task.await.unwrap(), End::Idle));
}

#[tokio::test(start_paused = true)]
async fn a_request_that_trickles_in_has_one_budget_not_one_per_byte() {
    let timeouts = Timeouts {
        frame: Duration::from_secs(30),
        ..Timeouts::default()
    };
    let (mut client, task, _stop) = start(timeouts, 4096);
    let request = api_versions(1);
    for byte in request.iter().take(4) {
        // The write that comes after the server has given up fails; that is the point.
        let _ = client.write_all(&[*byte]).await;
        sleep(Duration::from_secs(12)).await;
    }
    // 48 seconds in, with a byte every 12: gone at 30 (a budget that restarted with each byte would still be
    // waiting).
    assert!(task.is_finished());
    assert!(matches!(task.await.unwrap(), End::Slow));
}

#[tokio::test(start_paused = true)]
async fn a_client_that_stops_reading_cannot_hold_the_connection() {
    let timeouts = Timeouts {
        write: Duration::from_secs(5),
        ..Timeouts::default()
    };
    // A pipe too small for the answer, and a client that never reads it.
    let (mut client, task, _stop) = start(timeouts, 16);
    client.write_all(&api_versions(1)).await.unwrap();
    assert!(matches!(task.await.unwrap(), End::Slow));
}

#[tokio::test]
async fn stopping_the_server_ends_a_connection_that_is_waiting_for_a_request() {
    let (_client, task, stop) = start(Timeouts::default(), 4096);
    stop.cancel();
    assert!(matches!(task.await.unwrap(), End::Stopped));
}

#[tokio::test]
async fn a_client_that_hangs_up_mid_request_is_an_error_and_between_requests_a_goodbye() {
    let (mut client, task, _stop) = start(Timeouts::default(), 4096);
    client.write_all(&api_versions(1)[..6]).await.unwrap();
    drop(client);
    assert!(matches!(task.await.unwrap(), End::Io(_)));
    let (client, task, _stop) = start(Timeouts::default(), 4096);
    drop(client);
    assert!(matches!(task.await.unwrap(), End::PeerClosed));
}

#[tokio::test(start_paused = true)]
async fn a_partial_request_left_in_the_buffer_has_the_frame_budget_not_the_idle_one() {
    let (mut client, task, _stop) = start(Timeouts::default(), 4096);
    // One request whole and the start of the next, in one write; the rest never comes.
    let both = [api_versions(1), api_versions(2).slice(..6)].concat();
    client.write_all(&both).await.unwrap();
    assert_eq!(read_answer(&mut client).await.0, 1);
    assert!(matches!(task.await.unwrap(), End::Slow));
}
