//! Every ApiVersions exchange recorded from Apache Kafka 4.3.1 (tests/data/README.md), replayed through
//! the request reader, `answer` and the response writer. Kafka 4.4.0, which the Kafka 4 profile now stands for, serves
//! ApiVersions to v5 (KIP-1242) where 4.3.1 served it to v4: its own row is read as 4.4.0 lists it (0 to 5), and the two
//! steps that asked for v5 as a version not served are left out (v5 is served: `tools/wire-tail-oracle`, `api-versions`).
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
    unstable: false,
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
            let key = r["ApiKey"].as_i64().unwrap();
            let max = r["MaxVersion"].as_i64().unwrap();
            // ApiVersions itself: 4.3.1 recorded 0 to 4, and 4.4.0 serves 0 to 5.
            let max = if key == 18 && max == 4 { 5 } else { max };
            (key, r["MinVersion"].as_i64().unwrap(), max)
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
            if step["version"] == 5 {
                continue;
            }
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
    // 25 client-software steps (the 26th is a Metadata request) and 4 of the 6 unsupported-version steps.
    assert_eq!(replayed, 29);
}
