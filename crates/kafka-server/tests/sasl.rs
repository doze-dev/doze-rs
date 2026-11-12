//! A broker with SASL/PLAIN on a real socket: a client logs in and works, a wrong login is answered and
//! closed, and a stranger gets nothing.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/sasl_client.rs"]
mod client;

use client::{ALICE, Client, metadata_for, start_with};
use kafka_protocol::protocol::StrBytes;
use kafka_wire::batch::build::BatchBuilder;
use kafka_wire::messages::fetch_request::{FetchPartition, FetchTopic};
use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_wire::messages::{
    ApiKey, FetchRequest, FetchResponse, MetadataResponse, ProduceRequest, ProduceResponse,
    TopicName,
};

#[tokio::test]
async fn a_client_that_logs_in_with_plain_can_produce_and_fetch() {
    let server = start_with(true).await;
    let mut c = Client::connect(&server).await;
    c.log_in(ALICE).await;
    // The first ask makes the topic (the answer to it is "unknown"); the second finds it.
    c.call::<_, MetadataResponse>(ApiKey::Metadata, 12, &metadata_for("t"))
        .await
        .unwrap();
    let made: MetadataResponse = c
        .call(ApiKey::Metadata, 12, &metadata_for("t"))
        .await
        .unwrap();
    assert_eq!(made.topics[0].error_code, 0);

    let mut partition = PartitionProduceData::default();
    partition.index = 0;
    partition.records = Some(BatchBuilder::new().records(3).build());
    let mut topic = TopicProduceData::default();
    topic.name = TopicName(StrBytes::from_static_str("t"));
    topic.partition_data.push(partition);
    let mut produce = ProduceRequest::default();
    produce.acks = 1;
    produce.timeout_ms = 5000;
    produce.topic_data.push(topic);
    let wrote: ProduceResponse = c.call(ApiKey::Produce, 9, &produce).await.unwrap();
    assert_eq!(wrote.responses[0].partition_responses[0].error_code, 0);

    let mut fp = FetchPartition::default();
    fp.partition = 0;
    fp.partition_max_bytes = 1 << 20;
    let mut ft = FetchTopic::default();
    ft.topic = TopicName(StrBytes::from_static_str("t"));
    ft.partitions.push(fp);
    let mut fetch = FetchRequest::default();
    fetch.max_bytes = 1 << 20;
    fetch.topics.push(ft);
    let read: FetchResponse = c.call(ApiKey::Fetch, 12, &fetch).await.unwrap();
    let records = read.responses[0].partitions[0].records.as_ref();
    assert!(
        records.is_some_and(|r| !r.is_empty()),
        "the records written come back"
    );
    server.shutdown().await;
}

#[tokio::test]
async fn the_sasl_apis_are_listed_only_by_a_broker_that_has_sasl() {
    for (sasl, listed) in [(true, true), (false, false)] {
        let server = start_with(sasl).await;
        let mut c = Client::connect(&server).await;
        let keys: Vec<i16> = c
            .versions()
            .await
            .unwrap()
            .api_keys
            .iter()
            .map(|k| k.api_key)
            .collect();
        for key in [ApiKey::SaslHandshake, ApiKey::SaslAuthenticate] {
            assert_eq!(
                keys.contains(&(key as i16)),
                listed,
                "{key:?} with sasl {sasl}"
            );
        }
        server.shutdown().await;
    }
}

#[tokio::test]
async fn a_broker_without_sasl_serves_from_the_first_request_and_closes_on_a_handshake() {
    let server = start_with(false).await;
    let mut c = Client::connect(&server).await;
    assert!(
        c.call::<_, MetadataResponse>(ApiKey::Metadata, 12, &metadata_for("t"))
            .await
            .is_some()
    );
    assert!(
        c.handshake("PLAIN").await.is_none(),
        "not offered, so not answered"
    );
    server.shutdown().await;
}

#[tokio::test]
async fn a_wrong_password_is_answered_then_the_connection_is_closed() {
    let server = start_with(true).await;
    let mut c = Client::connect(&server).await;
    assert_eq!(c.handshake("PLAIN").await.unwrap().mechanisms.len(), 1);
    let refused = c.token(b"\0alice\0nope").await.unwrap();
    assert_eq!(refused.error_code, 58);
    assert_eq!(
        refused.error_message.as_deref(),
        Some("Authentication failed: Invalid username or password")
    );
    assert!(c.is_closed().await);
    server.shutdown().await;
}

#[tokio::test]
async fn an_unknown_mechanism_is_answered_with_the_list_then_closed() {
    let server = start_with(true).await;
    let mut c = Client::connect(&server).await;
    let r = c.handshake("SCRAM-SHA-256").await.unwrap();
    assert_eq!(r.error_code, 33);
    assert_eq!(
        r.mechanisms.iter().map(|m| &**m).collect::<Vec<_>>(),
        ["PLAIN"]
    );
    assert!(c.is_closed().await);
    server.shutdown().await;
}

#[tokio::test]
async fn a_request_before_the_login_is_closed_without_an_answer() {
    let server = start_with(true).await;
    let mut c = Client::connect(&server).await;
    c.send(ApiKey::Metadata, 12, &metadata_for("t")).await;
    assert!(c.is_closed().await);
    // no-handshake: a token first is closed too.
    let mut c = Client::connect(&server).await;
    assert!(c.token(ALICE).await.is_none());
    server.shutdown().await;
}

#[tokio::test]
async fn a_second_login_is_illegal_state_and_the_connection_and_its_user_stay() {
    let server = start_with(true).await;
    let mut c = Client::connect(&server).await;
    c.log_in(ALICE).await;
    let again = c.token(b"\0bob\0bob-secret-planted-91c2").await.unwrap();
    assert_eq!(again.error_code, 34);
    assert_eq!(
        again.error_message.as_deref(),
        Some("SaslAuthenticate request received after successful authentication")
    );
    let hs = c.handshake("PLAIN").await.unwrap();
    assert_eq!((hs.error_code, hs.mechanisms.len()), (34, 0));
    assert!(
        c.versions().await.is_some_and(|v| v.error_code == 0),
        "still served"
    );
    server.shutdown().await;
}

#[tokio::test]
async fn apiversions_before_the_handshake_is_answered_once_and_after_it_is_illegal_state() {
    let server = start_with(true).await;
    let mut c = Client::connect(&server).await;
    assert!(!c.versions().await.unwrap().api_keys.is_empty());
    assert!(
        c.versions().await.is_none(),
        "a second ApiVersions is closed unanswered"
    );

    let mut c = Client::connect(&server).await;
    assert_eq!(c.handshake("PLAIN").await.unwrap().error_code, 0);
    let mid = c.versions().await.unwrap();
    assert_eq!((mid.error_code, mid.api_keys.len()), (34, 0));
    assert!(c.is_closed().await, "and the login is over");
    server.shutdown().await;
}
