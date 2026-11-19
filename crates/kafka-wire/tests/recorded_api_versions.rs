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

/// Every profile starts each API where the real broker of its major starts it, except the floors W10 pins: the two
/// record-format ones (Produce v3 and Fetch v4, the first versions with record batch format v2) and six versions KIP-896
/// removed that a client negotiates past (ListOffsets v0, OffsetCommit v0 and v1, OffsetFetch v0, CreateTopics v0 and
/// v1, DeleteTopics v0, DescribeConfigs v0). Kafka 1.x has only v0 of the three ACL APIs, of OffsetForLeaderEpoch and of
/// DescribeLogDirs, and every major serves SaslHandshake v0 (raw tokens): the wire crate reads those versions
/// (vendor/kafka-protocol PATCHES.md 5) and the broker serves them (W10, SEC-401).
#[test]
fn every_profile_starts_where_the_real_broker_starts() {
    use kafka_protocol::messages::ApiKey;
    let floors = [
        ApiKey::Produce,
        ApiKey::Fetch,
        ApiKey::ListOffsets,
        ApiKey::OffsetCommit,
        ApiKey::OffsetFetch,
        ApiKey::CreateTopics,
        ApiKey::DeleteTopics,
        ApiKey::DescribeConfigs,
    ];
    for major in KafkaMajor::RANGE {
        let theirs = real(u64::from(major));
        for a in Profile::new(KafkaMajor::new(major).unwrap()).advertised(AS_RECORDED) {
            if floors.contains(&a.key) {
                continue;
            }
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
    /// Produce from v3 and Fetch from v4: the first versions with record batch format v2, the only format the log
    /// stores. wire.md W10.
    FormatFloor,
    /// A version KIP-896 removed (Kafka 4.0) and the wire crate does not read; every client negotiates past it.
    /// wire.md W10.
    Kip896Floor,
    /// An inter-broker or controller API that real brokers list on the client listener. Owner's decision, 2026-11-18: they
    /// stay out; clients never send them, a single node has no other broker, and serving them would only make the list
    /// match (wire.md W36, debt.md K4).
    InterBroker,
    /// Versions newer than the broker implements at Kafka 4. debt.md K1.
    NewerNotServed,
    /// Kafka 4.4.0's Produce v13 and TxnOffsetCommit v6: in flight, branch kafka-wire-tail (removed when it merges).
    InFlight,
    /// The column of an older major stops below the recorded broker of that major, or leaves out an API it lists
    /// (the columns were written for an earlier release of that major than the one recorded). debt.md K2.
    OlderColumn,
}

use Why::{
    FormatFloor as FORMAT_FLOOR, InFlight as IN_FLIGHT, InterBroker as INTER_BROKER,
    Kip896Floor as KIP896_FLOOR, NewerNotServed as NEWER_NOT_SERVED, OlderColumn as OLDER_COLUMN,
};

/// (major, API key, what the profile offers, why) for every difference from the recording. The list as it stood on
/// main before 2026-11-18 had 16 more: SaslHandshake v0 in all four majors, and v0 of the ACL APIs, OffsetForLeaderEpoch
/// and DescribeLogDirs in majors 1 to 3 (I2, SEC-D3: served since).
/// (major, API key, what the profile offers, why).
type Deviation = (u8, i16, Option<(i16, i16)>, &'static [Why]);

#[rustfmt::skip]
const DEVIATIONS: &[Deviation] = &[
    (1, 0, Some((3, 5)), &[FORMAT_FLOOR]), // Produce, recorded 0-5
    (1, 1, Some((4, 6)), &[FORMAT_FLOOR, OLDER_COLUMN]), // Fetch, recorded 0-7
    (1, 2, Some((1, 2)), &[KIP896_FLOOR]), // ListOffsets, recorded 0-2
    (1, 4, None, &[INTER_BROKER]), // LeaderAndISR, recorded 0-1
    (1, 5, None, &[INTER_BROKER]), // StopReplica, recorded 0-0
    (1, 6, None, &[INTER_BROKER]), // UpdateMetadata, recorded 0-4
    (1, 7, None, &[INTER_BROKER]), // ControlledShutdown, recorded 0-1
    (1, 8, Some((2, 3)), &[KIP896_FLOOR]), // OffsetCommit, recorded 0-3
    (1, 9, Some((1, 3)), &[KIP896_FLOOR]), // OffsetFetch, recorded 0-3
    (1, 19, Some((2, 2)), &[KIP896_FLOOR]), // CreateTopics, recorded 0-2
    (1, 20, Some((1, 1)), &[KIP896_FLOOR]), // DeleteTopics, recorded 0-1
    (1, 27, None, &[INTER_BROKER]), // WriteTxnMarkers, recorded 0-0
    (1, 32, Some((1, 1)), &[KIP896_FLOOR]), // DescribeConfigs, recorded 0-1
    (2, 0, Some((3, 7)), &[FORMAT_FLOOR, OLDER_COLUMN]), // Produce, recorded 0-9
    (2, 1, Some((4, 10)), &[FORMAT_FLOOR, OLDER_COLUMN]), // Fetch, recorded 0-12
    (2, 2, Some((1, 5)), &[KIP896_FLOOR, OLDER_COLUMN]), // ListOffsets, recorded 0-6
    (2, 3, Some((0, 8)), &[OLDER_COLUMN]), // Metadata, recorded 0-11
    (2, 4, None, &[INTER_BROKER]), // LeaderAndISR, recorded 0-5
    (2, 5, None, &[INTER_BROKER]), // StopReplica, recorded 0-3
    (2, 6, None, &[INTER_BROKER]), // UpdateMetadata, recorded 0-7
    (2, 7, None, &[INTER_BROKER]), // ControlledShutdown, recorded 0-3
    (2, 8, Some((2, 6)), &[KIP896_FLOOR, OLDER_COLUMN]), // OffsetCommit, recorded 0-8
    (2, 9, Some((1, 5)), &[KIP896_FLOOR, OLDER_COLUMN]), // OffsetFetch, recorded 0-7
    (2, 10, Some((0, 2)), &[OLDER_COLUMN]), // FindCoordinator, recorded 0-3
    (2, 11, Some((0, 4)), &[OLDER_COLUMN]), // JoinGroup, recorded 0-7
    (2, 12, Some((0, 2)), &[OLDER_COLUMN]), // Heartbeat, recorded 0-4
    (2, 13, Some((0, 2)), &[OLDER_COLUMN]), // LeaveGroup, recorded 0-4
    (2, 14, Some((0, 3)), &[OLDER_COLUMN]), // SyncGroup, recorded 0-5
    (2, 15, Some((0, 3)), &[OLDER_COLUMN]), // DescribeGroups, recorded 0-5
    (2, 16, Some((0, 2)), &[OLDER_COLUMN]), // ListGroups, recorded 0-4
    (2, 18, Some((0, 2)), &[OLDER_COLUMN]), // ApiVersions, recorded 0-3
    (2, 19, Some((2, 3)), &[KIP896_FLOOR, OLDER_COLUMN]), // CreateTopics, recorded 0-7
    (2, 20, Some((1, 3)), &[KIP896_FLOOR, OLDER_COLUMN]), // DeleteTopics, recorded 0-6
    (2, 21, Some((0, 1)), &[OLDER_COLUMN]), // DeleteRecords, recorded 0-2
    (2, 22, Some((0, 1)), &[OLDER_COLUMN]), // InitProducerID, recorded 0-4
    (2, 24, Some((0, 1)), &[OLDER_COLUMN]), // AddPartitionsToTxn, recorded 0-3
    (2, 25, Some((0, 1)), &[OLDER_COLUMN]), // AddOffsetsToTxn, recorded 0-3
    (2, 26, Some((0, 1)), &[OLDER_COLUMN]), // EndTxn, recorded 0-3
    (2, 27, None, &[INTER_BROKER]), // WriteTxnMarkers, recorded 0-1
    (2, 28, Some((0, 2)), &[OLDER_COLUMN]), // TxnOffsetCommit, recorded 0-3
    (2, 29, Some((0, 1)), &[OLDER_COLUMN]), // DescribeACLs, recorded 0-2
    (2, 30, Some((0, 1)), &[OLDER_COLUMN]), // CreateACLs, recorded 0-2
    (2, 31, Some((0, 1)), &[OLDER_COLUMN]), // DeleteACLs, recorded 0-2
    (2, 32, Some((1, 2)), &[KIP896_FLOOR, OLDER_COLUMN]), // DescribeConfigs, recorded 0-4
    (2, 37, Some((0, 1)), &[OLDER_COLUMN]), // CreatePartitions, recorded 0-3
    (2, 56, None, &[INTER_BROKER]), // AlterPartition, recorded 0-0
    (2, 60, None, &[OLDER_COLUMN]), // DescribeCluster, recorded 0-0
    (2, 61, None, &[OLDER_COLUMN]), // DescribeProducers, recorded 0-0
    (3, 0, Some((3, 11)), &[FORMAT_FLOOR]), // Produce, recorded 0-11
    (3, 1, Some((4, 13)), &[FORMAT_FLOOR, OLDER_COLUMN]), // Fetch, recorded 0-17
    (3, 2, Some((1, 7)), &[KIP896_FLOOR, OLDER_COLUMN]), // ListOffsets, recorded 0-9
    (3, 8, Some((2, 8)), &[KIP896_FLOOR, OLDER_COLUMN]), // OffsetCommit, recorded 0-9
    (3, 9, Some((1, 8)), &[KIP896_FLOOR, OLDER_COLUMN]), // OffsetFetch, recorded 0-9
    (3, 10, Some((0, 4)), &[OLDER_COLUMN]), // FindCoordinator, recorded 0-6
    (3, 11, Some((0, 7)), &[OLDER_COLUMN]), // JoinGroup, recorded 0-9
    (3, 13, Some((0, 4)), &[OLDER_COLUMN]), // LeaveGroup, recorded 0-5
    (3, 14, Some((0, 4)), &[OLDER_COLUMN]), // SyncGroup, recorded 0-5
    (3, 16, Some((0, 4)), &[OLDER_COLUMN]), // ListGroups, recorded 0-5
    (3, 18, Some((0, 3)), &[OLDER_COLUMN]), // ApiVersions, recorded 0-4
    (3, 19, Some((2, 7)), &[KIP896_FLOOR]), // CreateTopics, recorded 0-7
    (3, 20, Some((1, 5)), &[KIP896_FLOOR, OLDER_COLUMN]), // DeleteTopics, recorded 0-6
    (3, 27, None, &[INTER_BROKER]), // WriteTxnMarkers, recorded 0-1
    (3, 32, Some((1, 4)), &[KIP896_FLOOR]), // DescribeConfigs, recorded 0-4
    (3, 55, None, &[INTER_BROKER]), // DescribeQuorum, recorded 0-2
    (3, 64, None, &[INTER_BROKER]), // UnregisterBroker, recorded 0-0
    (3, 68, None, &[OLDER_COLUMN]), // ConsumerGroupHeartbeat, recorded 0-0
    (3, 69, None, &[OLDER_COLUMN]), // ConsumerGroupDescribe, recorded 0-0
    (3, 80, None, &[INTER_BROKER]), // AddRaftVoter, recorded 0-0
    (3, 81, None, &[INTER_BROKER]), // RemoveRaftVoter, recorded 0-0
    (4, 0, Some((3, 12)), &[FORMAT_FLOOR, IN_FLIGHT]), // Produce, recorded 0-13
    (4, 1, Some((4, 13)), &[NEWER_NOT_SERVED]), // Fetch, recorded 4-18
    (4, 2, Some((1, 7)), &[NEWER_NOT_SERVED]), // ListOffsets, recorded 1-11
    (4, 3, Some((0, 12)), &[NEWER_NOT_SERVED]), // Metadata, recorded 0-13
    (4, 8, Some((2, 9)), &[NEWER_NOT_SERVED]), // OffsetCommit, recorded 2-10
    (4, 9, Some((1, 9)), &[NEWER_NOT_SERVED]), // OffsetFetch, recorded 1-10
    (4, 10, Some((0, 4)), &[NEWER_NOT_SERVED]), // FindCoordinator, recorded 0-6
    (4, 15, Some((0, 5)), &[NEWER_NOT_SERVED]), // DescribeGroups, recorded 0-6
    (4, 18, Some((0, 4)), &[NEWER_NOT_SERVED]), // ApiVersions, recorded 0-5
    (4, 27, None, &[INTER_BROKER]), // WriteTxnMarkers, recorded 1-2
    (4, 28, Some((0, 5)), &[IN_FLIGHT]), // TxnOffsetCommit, recorded 0-6
    (4, 35, Some((1, 4)), &[NEWER_NOT_SERVED]), // DescribeLogDirs, recorded 1-5
    (4, 42, Some((0, 2)), &[NEWER_NOT_SERVED]), // DeleteGroups, recorded 0-3
    (4, 55, None, &[INTER_BROKER]), // DescribeQuorum, recorded 0-2
    (4, 64, None, &[INTER_BROKER]), // UnregisterBroker, recorded 0-0
    (4, 66, Some((0, 1)), &[NEWER_NOT_SERVED]), // ListTransactions, recorded 0-2
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
