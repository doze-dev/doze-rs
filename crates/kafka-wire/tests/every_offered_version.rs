//! The phase's "round trip against Kafka's message specs" gate: for every API a profile offers, at every
//! version it offers, a default request written by the crate's encoder is read by `incoming::read`, and a
//! default response written by `response::encode` is read back by the crate's decoder with no bytes left.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use bytes::Bytes;
use kafka_protocol::messages::*;
use kafka_protocol::protocol::Decodable;
use kafka_wire::api::Response;
use kafka_wire::incoming::{self, Incoming};
use kafka_wire::profile::{ApiSet, Features, KafkaMajor, Profile};
use kafka_wire::testing::request_payload;
use kafka_wire::{profile::Advertised, response};

const ALL: Features = Features {
    sasl: true,
    telemetry: true,
    served: ApiSet::ALL,
};

/// Builds, for every served API, the default request payload and the default response, by name.
macro_rules! defaults {
    ($($api:ident($req:ident, $resp:ident)),* $(,)?) => {
        fn request(key: ApiKey, version: i16) -> Bytes {
            match key {
                $(ApiKey::$api => request_payload(key, version, 7, Some("client"), &$req::default()),)*
                other => panic!("{other:?} is not a served API"),
            }
        }
        /// The default response of `key`, and a function that reads it back at a version.
        fn response_of(key: ApiKey) -> (Response, fn(&mut Bytes, i16)) {
            match key {
                $(ApiKey::$api => (Response::$api($resp::default()), |b, v| { $resp::decode(b, v).unwrap(); }),)*
                other => panic!("{other:?} is not a served API"),
            }
        }
    };
}

defaults!(
    Produce(ProduceRequest, ProduceResponse),
    Fetch(FetchRequest, FetchResponse),
    ListOffsets(ListOffsetsRequest, ListOffsetsResponse),
    Metadata(MetadataRequest, MetadataResponse),
    OffsetCommit(OffsetCommitRequest, OffsetCommitResponse),
    OffsetFetch(OffsetFetchRequest, OffsetFetchResponse),
    FindCoordinator(FindCoordinatorRequest, FindCoordinatorResponse),
    JoinGroup(JoinGroupRequest, JoinGroupResponse),
    Heartbeat(HeartbeatRequest, HeartbeatResponse),
    LeaveGroup(LeaveGroupRequest, LeaveGroupResponse),
    SyncGroup(SyncGroupRequest, SyncGroupResponse),
    DescribeGroups(DescribeGroupsRequest, DescribeGroupsResponse),
    ListGroups(ListGroupsRequest, ListGroupsResponse),
    SaslHandshake(SaslHandshakeRequest, SaslHandshakeResponse),
    ApiVersions(ApiVersionsRequest, ApiVersionsResponse),
    CreateTopics(CreateTopicsRequest, CreateTopicsResponse),
    DeleteTopics(DeleteTopicsRequest, DeleteTopicsResponse),
    DeleteRecords(DeleteRecordsRequest, DeleteRecordsResponse),
    InitProducerId(InitProducerIdRequest, InitProducerIdResponse),
    AddPartitionsToTxn(AddPartitionsToTxnRequest, AddPartitionsToTxnResponse),
    AddOffsetsToTxn(AddOffsetsToTxnRequest, AddOffsetsToTxnResponse),
    EndTxn(EndTxnRequest, EndTxnResponse),
    TxnOffsetCommit(TxnOffsetCommitRequest, TxnOffsetCommitResponse),
    DescribeAcls(DescribeAclsRequest, DescribeAclsResponse),
    CreateAcls(CreateAclsRequest, CreateAclsResponse),
    DeleteAcls(DeleteAclsRequest, DeleteAclsResponse),
    DescribeUserScramCredentials(
        DescribeUserScramCredentialsRequest,
        DescribeUserScramCredentialsResponse
    ),
    AlterUserScramCredentials(
        AlterUserScramCredentialsRequest,
        AlterUserScramCredentialsResponse
    ),
    DescribeConfigs(DescribeConfigsRequest, DescribeConfigsResponse),
    AlterConfigs(AlterConfigsRequest, AlterConfigsResponse),
    SaslAuthenticate(SaslAuthenticateRequest, SaslAuthenticateResponse),
    CreatePartitions(CreatePartitionsRequest, CreatePartitionsResponse),
    DeleteGroups(DeleteGroupsRequest, DeleteGroupsResponse),
    IncrementalAlterConfigs(
        IncrementalAlterConfigsRequest,
        IncrementalAlterConfigsResponse
    ),
    OffsetDelete(OffsetDeleteRequest, OffsetDeleteResponse),
    DescribeCluster(DescribeClusterRequest, DescribeClusterResponse),
    DescribeProducers(DescribeProducersRequest, DescribeProducersResponse),
    DescribeTransactions(DescribeTransactionsRequest, DescribeTransactionsResponse),
    ListTransactions(ListTransactionsRequest, ListTransactionsResponse),
    ConsumerGroupHeartbeat(
        ConsumerGroupHeartbeatRequest,
        ConsumerGroupHeartbeatResponse
    ),
    ConsumerGroupDescribe(ConsumerGroupDescribeRequest, ConsumerGroupDescribeResponse),
    GetTelemetrySubscriptions(
        GetTelemetrySubscriptionsRequest,
        GetTelemetrySubscriptionsResponse
    ),
    PushTelemetry(PushTelemetryRequest, PushTelemetryResponse),
);

/// Every (profile, API, version) the broker offers.
fn offered() -> Vec<(Profile, Advertised, i16)> {
    let mut out = Vec::new();
    for major in KafkaMajor::RANGE {
        let profile = Profile::new(KafkaMajor::new(major).unwrap());
        for adv in profile.advertised(ALL) {
            out.extend((adv.min..=adv.max).map(|v| (profile, adv, v)));
        }
    }
    out
}

#[test]
fn every_offered_request_is_read() {
    let triples = offered();
    assert!(triples.len() > 400, "{} triples", triples.len());
    for (profile, adv, version) in triples {
        let what = format!(
            "{:?} v{version} at major {}",
            adv.key,
            profile.major().get()
        );
        match incoming::read(request(adv.key, version), profile, ALL) {
            Ok(Incoming::Request { head, request }) => {
                assert_eq!(request.api_key(), adv.key, "{what}");
                assert_eq!((head.version, head.correlation_id), (version, 7), "{what}");
                assert_eq!(head.client_id.as_deref(), Some("client"), "{what}");
            }
            Ok(Incoming::ApiVersions { body, .. }) => assert!(body.is_some(), "{what}"),
            Ok(other) => panic!("{what}: {other:?}"),
            Err(e) => panic!("{what}: {e}"),
        }
    }
}

#[test]
fn every_offered_response_is_written_and_read_back() {
    for (profile, adv, version) in offered() {
        let what = format!(
            "{:?} v{version} at major {}",
            adv.key,
            profile.major().get()
        );
        let (resp, read_back) = response_of(adv.key);
        let frame = response::encode(7, version, &resp).unwrap();
        let mut rest = frame.slice(4..);
        let header_version = adv.key.response_header_version(version);
        assert_eq!(
            ResponseHeader::decode(&mut rest, header_version)
                .unwrap()
                .correlation_id,
            7,
            "{what}"
        );
        read_back(&mut rest, version);
        assert!(rest.is_empty(), "{what}: {} bytes left", rest.len());
    }
}

/// Writes the fuzzers' seed corpus: one valid request per API at its newest version, and a few valid
/// batches, so the fuzzers start from inputs that get past the header. Run on demand:
/// `cargo test -p kafka-wire --test every_offered_version -- --ignored write_the_seed_corpus`.
#[test]
#[ignore = "writes files; run by hand when the API table changes"]
fn write_the_seed_corpus() {
    use kafka_wire::batch::build::BatchBuilder;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/seeds");
    let (requests, batches, frames) = (
        root.join("fuzz_request"),
        root.join("fuzz_batch"),
        root.join("fuzz_frame"),
    );
    for dir in [&requests, &batches, &frames] {
        std::fs::create_dir_all(dir).unwrap();
    }
    for key in [ApiKey::ApiVersions, ApiKey::Metadata, ApiKey::Produce] {
        let newest = Profile::new(KafkaMajor::DEFAULT).range(key).unwrap().1;
        let framed = kafka_wire::testing::framed(&request(key, newest));
        std::fs::write(frames.join(format!("{key:?}.bin")), framed).unwrap();
    }
    let newest = Profile::new(KafkaMajor::DEFAULT);
    for adv in newest.advertised(ALL) {
        let name = format!("{:02}-{:?}-v{}.bin", adv.key as i16, adv.key, adv.max);
        std::fs::write(requests.join(name), request(adv.key, adv.max)).unwrap();
    }
    let seeds = [
        ("plain", BatchBuilder::new().records(3).build()),
        ("idempotent", BatchBuilder::new().producer(7, 1, 0).build()),
        (
            "gzip",
            BatchBuilder::new().attributes(1).body(vec![0; 16]).build(),
        ),
        (
            "transactional",
            BatchBuilder::new().attributes(0x10).build(),
        ),
    ];
    for (name, bytes) in seeds {
        std::fs::write(batches.join(format!("{name}.bin")), bytes).unwrap();
    }
}
