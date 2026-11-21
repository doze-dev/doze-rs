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
    unstable: false,
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
        unstable: false,
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
                unstable: false,
                served: ApiSet::ALL,
            }),
            p.advertised(AS_RECORDED)
        );
    }
}

/// Every profile starts each API where the real broker of its major starts it (debt K2, 2026-11-19): Produce v0 to v2 and
/// Fetch v0 to v3 carry message formats v0 and v1, converted to and from record batch format v2 (wire.md W38), and the
/// versions KIP-896 removed are read again (vendor/kafka-protocol PATCHES.md 5, 7 and 8). Kafka 1.x has only v0 of the three
/// ACL APIs, of OffsetForLeaderEpoch and of DescribeLogDirs, and every major serves SaslHandshake v0 (raw tokens, SEC-401).
#[test]
fn every_profile_starts_where_the_real_broker_starts() {
    use kafka_protocol::messages::ApiKey;
    for major in KafkaMajor::RANGE {
        let theirs = real(u64::from(major));
        for a in Profile::new(KafkaMajor::new(major).unwrap()).advertised(AS_RECORDED) {
            let key = i64::from(a.key as i16);
            let (_, min, _) = *theirs.iter().find(|t| t.0 == key).unwrap();
            assert_eq!(
                i64::from(a.min),
                min,
                "major {major}: {:?} starts at {} where the real broker starts at {min}",
                a.key,
                a.min
            );
        }
    }
    // And Kafka 1 offers the five APIs whose v0 is all it has.
    let one = Profile::new(KafkaMajor::new(1).unwrap());
    for key in [
        ApiKey::DescribeAcls,
        ApiKey::CreateAcls,
        ApiKey::DeleteAcls,
        ApiKey::OffsetForLeaderEpoch,
        ApiKey::DescribeLogDirs,
    ] {
        assert_eq!(one.range(key), Some((0, 0)), "{key:?} at --kafka-version 1");
    }
}

// --- Exact: every recorded API and range is offered as recorded, or is a written-down deviation (W36) ---

/// Why a profile differs from the recorded broker of its major, and the row that says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Why {
    /// An inter-broker or controller API that real brokers list on the client listener. Owner's decision, 2026-11-18: they
    /// stay out; clients never send them, a single node has no other broker, and serving them would only make the list
    /// match (wire.md W36, debt.md K4).
    InterBroker,
}

use Why::InterBroker as INTER_BROKER;

/// (major, API key, what the profile offers, why) for every difference from the recording. The list as it stood on
/// main before 2026-11-18 had 16 more: SaslHandshake v0 in all four majors, and v0 of the ACL APIs, OffsetForLeaderEpoch
/// and DescribeLogDirs in majors 1 to 3 (I2, SEC-D3: served since), and on 2026-11-19 twelve more at Kafka 4, every version
/// 4.4.0 lists of an API served (K1: Produce 13, Fetch 18, ListOffsets 11, Metadata 13, OffsetCommit and OffsetFetch 10,
/// FindCoordinator 6, DescribeGroups 6, ApiVersions 5, TxnOffsetCommit 6, DescribeLogDirs 5, DeleteGroups 3, ListTransactions 2),
/// and on 2026-11-19 the rest but K4 (K2): the floors of Produce, Fetch and the versions KIP-896 removed in majors 1 to 3 and
/// Produce's at Kafka 4, and the columns of majors 1 to 3 raised to the recorded 1.1, 2.8 and 3.9 (about thirty maxima, and
/// DescribeCluster and DescribeProducers at 2.8, ConsumerGroupHeartbeat and ConsumerGroupDescribe at 3.9).
/// (major, API key, what the profile offers, why).
type Deviation = (u8, i16, Option<(i16, i16)>, &'static [Why]);

#[rustfmt::skip]
const DEVIATIONS: &[Deviation] = &[
    (1, 4, None, &[INTER_BROKER]), // LeaderAndISR, recorded 0-1
    (1, 5, None, &[INTER_BROKER]), // StopReplica, recorded 0-0
    (1, 6, None, &[INTER_BROKER]), // UpdateMetadata, recorded 0-4
    (1, 7, None, &[INTER_BROKER]), // ControlledShutdown, recorded 0-1
    (1, 27, None, &[INTER_BROKER]), // WriteTxnMarkers, recorded 0-0
    (2, 4, None, &[INTER_BROKER]), // LeaderAndISR, recorded 0-5
    (2, 5, None, &[INTER_BROKER]), // StopReplica, recorded 0-3
    (2, 6, None, &[INTER_BROKER]), // UpdateMetadata, recorded 0-7
    (2, 7, None, &[INTER_BROKER]), // ControlledShutdown, recorded 0-3
    (2, 27, None, &[INTER_BROKER]), // WriteTxnMarkers, recorded 0-1
    (2, 56, None, &[INTER_BROKER]), // AlterPartition, recorded 0-0
    (3, 27, None, &[INTER_BROKER]), // WriteTxnMarkers, recorded 0-1
    (3, 55, None, &[INTER_BROKER]), // DescribeQuorum, recorded 0-2
    (3, 64, None, &[INTER_BROKER]), // UnregisterBroker, recorded 0-0
    (3, 80, None, &[INTER_BROKER]), // AddRaftVoter, recorded 0-0
    (3, 81, None, &[INTER_BROKER]), // RemoveRaftVoter, recorded 0-0
    (4, 27, None, &[INTER_BROKER]), // WriteTxnMarkers, recorded 1-2
    (4, 55, None, &[INTER_BROKER]), // DescribeQuorum, recorded 0-2
    (4, 64, None, &[INTER_BROKER]), // UnregisterBroker, recorded 0-0
    (4, 80, None, &[INTER_BROKER]), // AddRaftVoter, recorded 0-1
    (4, 81, None, &[INTER_BROKER]), // RemoveRaftVoter, recorded 0-0
    (4, 83, None, &[INTER_BROKER]), // InitializeShareGroupState, recorded 0-0
    (4, 84, None, &[INTER_BROKER]), // ReadShareGroupState, recorded 0-0
    (4, 85, None, &[INTER_BROKER]), // WriteShareGroupState, recorded 0-1
    (4, 86, None, &[INTER_BROKER]), // DeleteShareGroupState, recorded 0-0
    (4, 87, None, &[INTER_BROKER]), // ReadShareGroupStateSummary, recorded 0-1
    (4, 94, None, &[INTER_BROKER]), // UnregisterController, recorded 0-0
];

#[test]
fn every_recorded_range_is_offered_as_recorded_or_is_a_written_down_deviation() {
    let mut unexplained = Vec::new();
    let mut used = vec![false; DEVIATIONS.len()];
    for major in KafkaMajor::RANGE {
        let theirs = real(u64::from(major));
        let ours: Vec<(i64, i64, i64)> = Profile::new(KafkaMajor::new(major).unwrap())
            .advertised(AS_RECORDED)
            .iter()
            .map(|a| (i64::from(a.key as i16), i64::from(a.min), i64::from(a.max)))
            .collect();
        let mut keys: Vec<i64> = theirs.iter().chain(&ours).map(|t| t.0).collect();
        keys.sort_unstable();
        keys.dedup();
        for key in keys {
            let r = theirs.iter().find(|t| t.0 == key).map(|t| (t.1, t.2));
            let o = ours.iter().find(|t| t.0 == key).map(|t| (t.1, t.2));
            if r == o {
                continue;
            }
            let found = DEVIATIONS.iter().position(|d| {
                d.0 == major
                    && i64::from(d.1) == key
                    && d.2.map(|(a, b)| (i64::from(a), i64::from(b))) == o
            });
            match found {
                Some(i) => used[i] = true,
                None => unexplained.push(format!(
                    "major {major} key {key}: recorded {r:?}, offered {o:?}"
                )),
            }
        }
    }
    let stale: Vec<_> = DEVIATIONS
        .iter()
        .zip(&used)
        .filter(|(_, u)| !**u)
        .map(|(d, _)| format!("major {} key {}: {:?}", d.0, d.1, d.3))
        .collect();
    assert!(
        unexplained.is_empty() && stale.is_empty(),
        "differences not in DEVIATIONS: {unexplained:#?}\nDEVIATIONS that no longer differ: {stale:#?}"
    );
}
