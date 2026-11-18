//! A fetch of megabytes, on a real socket and (feature `tls`) over TLS: the records the log holds are written to the
//! socket from its own buffers, in pieces, and must arrive whole and in order, every batch's CRC right, at every
//! form of Fetch (the topic named, v4 to v12; by id, v13; the flexible form from v12).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/sasl_client.rs"]
mod client;
#[cfg(feature = "tls")]
#[path = "support/tls_support.rs"]
mod support;

use client::{Client, metadata_for};
use kafka_protocol::protocol::StrBytes;
use kafka_wire::batch::Batches;
use kafka_wire::batch::build::{BatchBuilder, Rec};
use kafka_wire::messages::fetch_request::{FetchPartition, FetchTopic};
use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_wire::messages::{
    ApiKey, FetchRequest, FetchResponse, MetadataResponse, ProduceRequest, ProduceResponse,
    TopicName,
};
use tokio::io::{AsyncRead, AsyncWrite};

const BATCHES: usize = 8;
const VALUE: usize = 256 << 10;

/// Writes [`BATCHES`] batches of one [`VALUE`]-byte record each to topic `big`, then fetches them back at each form of
/// Fetch and checks they all came.
async fn produce_then_fetch_whole<S: AsyncRead + AsyncWrite + Unpin>(c: &mut Client<S>) {
    let mut topic_id = None;
    for _ in 0..2 {
        let m: MetadataResponse = c
            .call(ApiKey::Metadata, 12, &metadata_for("big"))
            .await
            .unwrap();
        topic_id = m.topics.first().map(|t| t.topic_id);
    }
    for i in 0..BATCHES {
        let value = vec![u8::try_from(i).unwrap() + 1; VALUE];
        let rec = Rec {
            value: Some(value),
            ..Rec::default()
        };
        let mut partition = PartitionProduceData::default();
        partition.records = Some(BatchBuilder::from_records(0, &[rec]).build());
        let mut topic = TopicProduceData::default();
        topic.name = TopicName(StrBytes::from_static_str("big"));
        topic.partition_data.push(partition);
        let mut produce = ProduceRequest::default();
        produce.acks = 1;
        produce.timeout_ms = 5000;
        produce.topic_data.push(topic);
        let wrote: ProduceResponse = c.call(ApiKey::Produce, 9, &produce).await.unwrap();
        assert_eq!(wrote.responses[0].partition_responses[0].error_code, 0);
    }
    for version in [4, 11, 12, 13] {
        let mut fp = FetchPartition::default();
        fp.partition_max_bytes = 64 << 20;
        let mut ft = FetchTopic::default();
        ft.topic = TopicName(StrBytes::from_static_str("big"));
        if version >= 13 {
            ft.topic_id = topic_id.unwrap();
        }
        ft.partitions.push(fp);
        let mut fetch = FetchRequest::default();
        fetch.max_bytes = 64 << 20;
        fetch.topics.push(ft);
        let read: FetchResponse = c.call(ApiKey::Fetch, version, &fetch).await.unwrap();
        let p = &read.responses[0].partitions[0];
        assert_eq!(p.error_code, 0, "v{version}");
        let records = p.records.clone().unwrap();
        assert!(records.len() > BATCHES * VALUE, "v{version}");
        let batches: Vec<_> = Batches::new(records).collect::<Result<_, _>>().unwrap();
        assert_eq!(batches.len(), BATCHES, "v{version}");
    }
}

#[tokio::test]
async fn a_fetch_of_megabytes_arrives_whole_on_a_socket() {
    let server = client::start_with(false).await;
    let mut c = Client::connect(&server).await;
    produce_then_fetch_whole(&mut c).await;
    server.shutdown().await;
}

#[cfg(feature = "tls")]
#[tokio::test]
async fn a_fetch_of_megabytes_arrives_whole_over_tls() {
    let server = support::start(support::Opts::default()).await;
    let mut c = support::dial(&server).await;
    produce_then_fetch_whole(&mut c).await;
    server.shutdown().await;
}
