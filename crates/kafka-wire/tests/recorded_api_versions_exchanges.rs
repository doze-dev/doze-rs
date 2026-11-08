//! Every ApiVersions exchange recorded from Apache Kafka 4.3.1 (tests/data/README.md), replayed through
//! the request reader, `answer` and the response writer.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use bytes::Bytes;
use kafka_protocol::messages::{ApiKey, ApiVersionsRequest, ApiVersionsResponse, ResponseHeader};
use kafka_protocol::protocol::{Decodable, StrBytes};
use kafka_wire::api::Response;
use kafka_wire::api_versions::answer;
use kafka_wire::incoming::{self, Incoming};
use kafka_wire::profile::{ApiSet, Features, KafkaMajor, Profile};
use kafka_wire::{response, testing::request_payload};
use serde_json::Value;

const RECORDED: [&str; 2] = [
    include_str!("data/apiversions-client-software.json"),
    include_str!("data/apiversions-unsupported-version.json"),
];

/// The recorded brokers list the SASL APIs and have no client telemetry reporter.
const AS_RECORDED: Features = Features {
    sasl: true,
    telemetry: false,
    served: ApiSet::ALL,
};

fn payload(step: &Value) -> Bytes {
    let version = i16::try_from(step["version"].as_i64().unwrap()).unwrap();
    if version <= 4 {
        let mut q = ApiVersionsRequest::default();
        let text = |k: &str| StrBytes::from_string(step["request"][k].as_str().unwrap().to_owned());
        q.client_software_name = text("ClientSoftwareName");
        q.client_software_version = text("ClientSoftwareVersion");
        return request_payload(ApiKey::ApiVersions, version, 9, Some("c"), &q);
    }
    // A version no encoder knows: a v2 header (null client id, no tagged fields) and a body that is never read.
    let mut raw = vec![0, 18];
    raw.extend_from_slice(&version.to_be_bytes());
    raw.extend_from_slice(&[0, 0, 0, 9, 0xff, 0xff, 0, 0xde, 0xad]);
    Bytes::from(raw)
}

fn rows(v: &Value) -> Vec<(i64, i64, i64)> {
    v["ApiKeys"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["ApiKey"].as_i64().unwrap(),
                r["MinVersion"].as_i64().unwrap(),
                r["MaxVersion"].as_i64().unwrap(),
            )
        })
        .collect()
}

#[test]
fn every_recorded_exchange_is_answered_as_kafka_answered() {
    let profile = Profile::new(KafkaMajor::DEFAULT);
    let mut replayed = 0;
    for file in RECORDED {
        let scenario: Value = serde_json::from_str(file).unwrap();
        for step in scenario["steps"].as_array().unwrap() {
            if step["api"] != "ApiVersions" {
                continue;
            }
            let label = step["label"].as_str().unwrap();
            let Incoming::ApiVersions { head, body } =
                incoming::read(payload(step), profile, AS_RECORDED).unwrap()
            else {
                panic!("{label}: not read as ApiVersions");
            };
            let (version, ours) = answer(profile, AS_RECORDED, head.version, body.as_ref());
            let recorded = &step["response"];
            assert_eq!(
                i64::from(ours.error_code),
                recorded["ErrorCode"].as_i64().unwrap(),
                "{label}"
            );
            let ours_rows: Vec<_> = ours
                .api_keys
                .iter()
                .map(|r| {
                    (
                        i64::from(r.api_key),
                        i64::from(r.min_version),
                        i64::from(r.max_version),
                    )
                })
                .collect();
            let theirs = rows(recorded);
            if recorded["OtherApiKeys"] == true {
                assert!(theirs.iter().all(|r| ours_rows.contains(r)), "{label}");
            } else {
                assert_eq!(ours_rows, theirs, "{label}");
            }
            // The frame is readable by the client at the version it is answered in, with nothing left over.
            let frame = response::encode(9, version, &Response::ApiVersions(ours)).unwrap();
            let mut rest = frame.slice(4..);
            assert_eq!(
                ResponseHeader::decode(&mut rest, 0).unwrap().correlation_id,
                9
            );
            ApiVersionsResponse::decode(&mut rest, version).unwrap();
            assert!(rest.is_empty(), "{label}");
            replayed += 1;
        }
    }
    // 25 client-software steps (the 26th is a Metadata request) and 6 unsupported-version steps.
    assert_eq!(replayed, 31);
}
