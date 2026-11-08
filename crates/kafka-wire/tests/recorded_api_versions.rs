//! No profile may offer more than a real broker of that major (tests/data/README.md).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use kafka_wire::profile::{ApiSet, Features, KafkaMajor, Profile};

fn recording() -> serde_json::Value {
    serde_json::from_str(include_str!("data/api-versions.json")).expect("the recording is JSON")
}

/// (key, min, max) of every API a real broker of `major` lists.
fn real(major: u64) -> Vec<(i64, i64, i64)> {
    let rec = recording();
    let broker = rec["brokers"]
        .as_array()
        .and_then(|b| b.iter().find(|b| b["major"] == major))
        .expect("a recorded broker for each major");
    broker["apis"]
        .as_array()
        .expect("apis")
        .iter()
        .map(|a| {
            (
                a["key"].as_i64().unwrap(),
                a["min"].as_i64().unwrap(),
                a["max"].as_i64().unwrap(),
            )
        })
        .collect()
}

/// The recorded brokers list SaslHandshake and SaslAuthenticate whether or not SASL is on, and have no client
/// telemetry reporter, so they do not list the two telemetry APIs. Those are an extension, offered only when
/// client telemetry is switched on (see `telemetry_apis_are_the_only_extension`).
const AS_RECORDED: Features = Features {
    sasl: true,
    telemetry: false,
    served: ApiSet::ALL,
};

#[test]
fn no_profile_offers_more_than_a_real_broker_of_that_major() {
    let all = AS_RECORDED;
    for major in KafkaMajor::RANGE {
        let theirs = real(u64::from(major));
        let ours = Profile::new(KafkaMajor::new(major).unwrap()).advertised(all);
        assert!(!ours.is_empty());
        for a in ours {
            let key = i64::from(a.key as i16);
            let Some(&(_, min, max)) = theirs.iter().find(|t| t.0 == key) else {
                panic!(
                    "major {major}: we offer {:?} and the real broker lists no such API",
                    a.key
                );
            };
            assert!(
                i64::from(a.min) >= min && i64::from(a.max) <= max,
                "major {major}: {:?} {}..{} is wider than the real {min}..{max}",
                a.key,
                a.min,
                a.max
            );
        }
    }
}

#[test]
fn each_major_has_a_recording() {
    for major in KafkaMajor::RANGE {
        assert!(!real(u64::from(major)).is_empty());
    }
}

#[test]
fn telemetry_apis_are_the_only_extension() {
    let major4 = Profile::new(KafkaMajor::DEFAULT);
    let with = major4.advertised(Features {
        sasl: true,
        telemetry: true,
        served: ApiSet::ALL,
    });
    let without = major4.advertised(AS_RECORDED);
    let extra: Vec<_> = with
        .iter()
        .filter(|a| !without.contains(a))
        .map(|a| a.key)
        .collect();
    assert_eq!(
        extra,
        [
            kafka_protocol::messages::ApiKey::GetTelemetrySubscriptions,
            kafka_protocol::messages::ApiKey::PushTelemetry
        ]
    );
    // Neither is offered before Kafka 4.
    for major in 1..=3 {
        let p = Profile::new(KafkaMajor::new(major).unwrap());
        assert_eq!(
            p.advertised(Features {
                sasl: true,
                telemetry: true,
                served: ApiSet::ALL,
            }),
            p.advertised(AS_RECORDED)
        );
    }
}
