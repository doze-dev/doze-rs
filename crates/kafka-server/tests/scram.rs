//! A broker with SASL/SCRAM on a real socket: a client logs in with either hash and works, a wrong password is
//! answered then closed, and a proof of the wrong length is closed without an answer.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/sasl_client.rs"]
mod client;

use client::{ALICE, BOB_SECRET, Client, SECRET, metadata_for, start_scram};
use kafka_auth::scram::Kind;
use kafka_auth::scram::client::Client as ScramClient;
use kafka_protocol::protocol::StrBytes;
use kafka_wire::batch::build::BatchBuilder;
use kafka_wire::messages::fetch_request::{FetchPartition, FetchTopic};
use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_wire::messages::{
    ApiKey, FetchRequest, FetchResponse, MetadataResponse, ProduceRequest, ProduceResponse,
    TopicName,
};

const BAD: &str =
    "Authentication failed during authentication due to invalid credentials with SASL mechanism ";

/// Produces three records to `topic` and reads them back.
async fn produce_and_fetch(c: &mut Client, topic: &str) {
    // The first ask makes the topic (the answer to it is "unknown"); the second finds it.
    for _ in 0..2 {
        c.call::<_, MetadataResponse>(ApiKey::Metadata, 12, &metadata_for(topic))
            .await
            .unwrap();
    }
    let mut partition = PartitionProduceData::default();
    partition.index = 0;
    partition.records = Some(BatchBuilder::new().records(3).build());
    let mut data = TopicProduceData::default();
    data.name = TopicName(StrBytes::from_string(topic.to_owned()));
    data.partition_data.push(partition);
    let mut produce = ProduceRequest::default();
    produce.acks = 1;
    produce.timeout_ms = 5000;
    produce.topic_data.push(data);
    let wrote: ProduceResponse = c.call(ApiKey::Produce, 9, &produce).await.unwrap();
    assert_eq!(wrote.responses[0].partition_responses[0].error_code, 0);

    let mut fp = FetchPartition::default();
    fp.partition = 0;
    fp.partition_max_bytes = 1 << 20;
    let mut ft = FetchTopic::default();
    ft.topic = TopicName(StrBytes::from_string(topic.to_owned()));
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
}

#[tokio::test]
async fn a_client_that_logs_in_with_scram_can_produce_and_fetch() {
    let server = start_scram().await;
    for (kind, topic) in [(Kind::Sha256, "t256"), (Kind::Sha512, "t512")] {
        let mut c = Client::connect(&server).await;
        c.scram_log_in(kind, "alice", SECRET).await;
        produce_and_fetch(&mut c, topic).await;
    }
    // PLAIN still works next to them, and bob is his own user.
    let mut c = Client::connect(&server).await;
    c.log_in(ALICE).await;
    let mut c = Client::connect(&server).await;
    c.scram_log_in(Kind::Sha512, "bob", BOB_SECRET).await;
    server.shutdown().await;
}

#[tokio::test]
async fn the_handshake_lists_plain_then_512_then_256_and_a_wrong_case_is_refused() {
    let server = start_scram().await;
    let mut c = Client::connect(&server).await;
    let r = c.handshake("SCRAM-SHA-256").await.unwrap();
    let names: Vec<_> = r.mechanisms.iter().map(|m| &**m).collect();
    assert_eq!(r.error_code, 0);
    assert_eq!(names, ["PLAIN", "SCRAM-SHA-512", "SCRAM-SHA-256"]);
    let mut c = Client::connect(&server).await;
    let r = c.handshake("scram-sha-256").await.unwrap();
    assert_eq!(r.error_code, 33);
    assert!(c.is_closed().await);
    server.shutdown().await;
}

#[tokio::test]
async fn a_wrong_password_is_answered_at_the_final_message_then_the_connection_is_closed() {
    let server = start_scram().await;
    for kind in [Kind::Sha256, Kind::Sha512] {
        let mut c = Client::connect(&server).await;
        let me = ScramClient::new(kind, "alice", "not-the-password", "client-nonce-0002.");
        let challenge = c.scram_first(kind, &me).await;
        assert_eq!(challenge.error_code, 0);
        let refused = c
            .token(&me.last(&challenge.auth_bytes, None, "").message)
            .await
            .unwrap();
        let text = format!("{BAD}{}", kind.mechanism());
        assert_eq!(
            (refused.error_code, refused.error_message.as_deref()),
            (58, Some(text.as_str()))
        );
        assert!(refused.auth_bytes.is_empty());
        assert!(c.is_closed().await);
    }
    server.shutdown().await;
}

#[tokio::test]
async fn an_unknown_user_is_answered_at_the_first_message_then_the_connection_is_closed() {
    let server = start_scram().await;
    let mut c = Client::connect(&server).await;
    let me = ScramClient::new(Kind::Sha256, "mallory", "whatever", "client-nonce-0003.");
    let refused = c.scram_first(Kind::Sha256, &me).await;
    let text = format!("{BAD}SCRAM-SHA-256");
    assert_eq!(
        (refused.error_code, refused.error_message.as_deref()),
        (58, Some(text.as_str()))
    );
    assert!(c.is_closed().await, "the final message is never read");
    server.shutdown().await;
}

#[tokio::test]
async fn a_proof_that_is_not_a_hash_long_closes_the_connection_without_an_answer() {
    let server = start_scram().await;
    for proof in ["AAAA", ""] {
        let mut c = Client::connect(&server).await;
        let me = ScramClient::new(Kind::Sha256, "alice", SECRET, "client-nonce-0004.");
        let challenge = c.scram_first(Kind::Sha256, &me).await;
        let text = String::from_utf8(challenge.auth_bytes.to_vec()).unwrap();
        let nonce = text.split(',').next().unwrap();
        let last = format!("c=biws,{nonce},p={proof}");
        assert!(
            c.token(last.as_bytes()).await.is_none(),
            "no answer for p={proof:?}"
        );
    }
    server.shutdown().await;
}

#[tokio::test]
async fn a_request_between_the_two_tokens_is_not_served_and_a_second_login_is_illegal() {
    let server = start_scram().await;
    let mut c = Client::connect(&server).await;
    let me = ScramClient::new(Kind::Sha512, "alice", SECRET, "client-nonce-0005.");
    c.scram_first(Kind::Sha512, &me).await;
    c.send(ApiKey::Metadata, 12, &metadata_for("t")).await;
    assert!(c.is_closed().await);

    let mut c = Client::connect(&server).await;
    c.scram_log_in(Kind::Sha512, "alice", SECRET).await;
    let again = c.token(b"n,,n=bob,r=x").await.unwrap();
    assert_eq!(again.error_code, 34);
    let hs = c.handshake("SCRAM-SHA-256").await.unwrap();
    assert_eq!((hs.error_code, hs.mechanisms.len()), (34, 0));
    server.shutdown().await;
}
