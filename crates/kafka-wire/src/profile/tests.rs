use kafka_protocol::messages::ApiKey;

use super::*;

fn profile(major: u8) -> Profile {
    Profile::new(KafkaMajor::new(major).unwrap())
}

/// KIP-890 by major, as each real broker offered it (`tests/data/api-versions.json`): Kafka 3.9 has the first part
/// (Produce v11, EndTxn v4, TxnOffsetCommit v4, AddPartitionsToTxn v5: the abortable error and the broker's verifying
/// request) and stays on the classic protocol; Kafka 4 adds the second (Produce v12, EndTxn v5, TxnOffsetCommit v5) and
/// finalizes `transaction.version` 2. Kafka 1 and 2 have neither. Kafka 4.4.0 names the topics of Produce (v13) and
/// TxnOffsetCommit (v6) by id too (KIP-516).
#[test]
fn only_kafka_4_offers_the_second_part_of_kip_890() {
    let max = |major: u8, key: ApiKey| profile(major).range(key).map(|(_, max)| max);
    assert_eq!(max(4, ApiKey::Produce), Some(13));
    assert_eq!(max(4, ApiKey::EndTxn), Some(5));
    assert_eq!(max(4, ApiKey::TxnOffsetCommit), Some(6));
    assert_eq!(max(4, ApiKey::AddPartitionsToTxn), Some(5));
    assert_eq!(max(3, ApiKey::Produce), Some(11));
    assert_eq!(max(3, ApiKey::EndTxn), Some(4));
    assert_eq!(max(3, ApiKey::TxnOffsetCommit), Some(4));
    assert_eq!(max(3, ApiKey::AddPartitionsToTxn), Some(5));
    assert_eq!(max(2, ApiKey::EndTxn), Some(1));
    assert_eq!(profile(4).transaction_version(), 2);
    for major in 1..=3 {
        assert_eq!(profile(major).transaction_version(), 0, "Kafka {major}");
    }
}

const ALL: Features = Features {
    sasl: true,
    telemetry: true,
    unstable: false,
    served: ApiSet::ALL,
};

#[test]
fn advertised_is_sorted_and_conditional() {
    let plain = profile(4).advertised(Features::default());
    assert!(
        plain
            .windows(2)
            .all(|w| (w[0].key as i16) < (w[1].key as i16))
    );
    let has = |l: &[Advertised], k| l.iter().any(|a| a.key == k);
    assert!(!has(&plain, ApiKey::SaslHandshake) && !has(&plain, ApiKey::PushTelemetry));
    let full = profile(4).advertised(ALL);
    assert!(has(&full, ApiKey::SaslHandshake) && has(&full, ApiKey::PushTelemetry));
    assert_eq!(full.len(), plain.len() + 4);
}

/// Share groups (KIP-932) at the versions Kafka 4.4.0 serves, and in no profile before Kafka 4 (`tests/data/api-versions.json`
/// has them for 4.4.0, the recorded major 4); `share.version` 2 is finalized where they are.
#[test]
fn only_kafka_4_offers_share_groups_at_kafka_4_4_versions() {
    let share = [
        (ApiKey::ShareGroupHeartbeat, (1, 1)),
        (ApiKey::ShareGroupDescribe, (1, 1)),
        (ApiKey::ShareFetch, (1, 2)),
        (ApiKey::ShareAcknowledge, (1, 2)),
        (ApiKey::DescribeShareGroupOffsets, (0, 1)),
        (ApiKey::AlterShareGroupOffsets, (0, 0)),
        (ApiKey::DeleteShareGroupOffsets, (0, 0)),
    ];
    for (key, range) in share {
        assert_eq!(profile(4).range(key), Some(range), "{key:?}");
        for major in 1..=3 {
            assert_eq!(profile(major).range(key), None, "{key:?} at Kafka {major}");
        }
    }
    assert_eq!(profile(4).share_version(), 2);
    assert_eq!(profile(3).share_version(), 0);
}

/// Streams groups (KIP-1071) at the versions Kafka 4.4.0 serves (heartbeat and describe 0 to 1, the topology description
/// update 0), and in no profile before Kafka 4; `streams.version` 1 is finalized where they are.
#[test]
fn only_kafka_4_offers_streams_groups_at_kafka_4_4_versions() {
    let streams = [
        (ApiKey::StreamsGroupHeartbeat, (0, 1)),
        (ApiKey::StreamsGroupDescribe, (0, 1)),
        (ApiKey::StreamsGroupTopologyDescriptionUpdate, (0, 0)),
    ];
    for (key, range) in streams {
        assert_eq!(profile(4).range(key), Some(range), "{key:?}");
        for major in 1..=3 {
            assert_eq!(profile(major).range(key), None, "{key:?} at Kafka {major}");
        }
    }
    assert_eq!(profile(4).streams_version(), 1);
    assert_eq!(profile(3).streams_version(), 0);
}

#[test]
fn each_major_offers_what_it_should() {
    assert!(!profile(1).offers_consumer_groups() && !profile(3).offers_consumer_groups());
    assert!(profile(4).offers_consumer_groups());
    assert_eq!(profile(1).range(ApiKey::DescribeCluster), None);
    assert!(profile(3).range(ApiKey::DescribeCluster).is_some());
    assert_eq!(profile(4).range(ApiKey::Fetch), Some((4, 18)));
    assert_eq!(profile(1).range(ApiKey::Produce), Some((3, 5)));
}

/// InitProducerId v6 (KIP-939, two-phase commit) is `latestVersionUnstable` in Kafka 4.4.0's spec: a broker with the
/// defaults offers 0 to 5 and refuses v6, one with `unstable.api.versions.enable` offers and serves 0 to 6. It is the only
/// unstable version, and no profile before Kafka 4 changes with the switch.
#[test]
fn init_producer_id_v6_is_offered_only_with_the_unstable_versions() {
    let unstable = Features {
        unstable: true,
        ..ALL
    };
    let p = profile(4);
    assert_eq!(p.range(ApiKey::InitProducerId), Some((0, 5)));
    let max = |f: Features| {
        p.advertised(f)
            .iter()
            .find(|a| a.key == ApiKey::InitProducerId)
            .map(|a| a.max)
    };
    assert_eq!(max(ALL), Some(5));
    assert_eq!(max(unstable), Some(6));
    assert!(p.check(ApiKey::InitProducerId, 6, ALL).is_err());
    assert!(p.check(ApiKey::InitProducerId, 5, ALL).is_ok());
    assert!(p.check(ApiKey::InitProducerId, 6, unstable).is_ok());
    assert!(p.check(ApiKey::InitProducerId, 7, unstable).is_err());
    let changed: Vec<_> = p
        .advertised(unstable)
        .into_iter()
        .filter(|a| !p.advertised(ALL).contains(a))
        .map(|a| a.key)
        .collect();
    assert_eq!(changed, [ApiKey::InitProducerId]);
    for major in 1..=3 {
        assert_eq!(
            profile(major).advertised(unstable),
            profile(major).advertised(ALL),
            "Kafka {major}"
        );
    }
}

#[test]
fn outside_the_range_is_not_served() {
    let p = profile(4);
    assert!(p.check(ApiKey::Fetch, 18, Features::default()).is_ok());
    assert!(p.check(ApiKey::Fetch, 3, Features::default()).is_err());
    assert!(p.check(ApiKey::Fetch, 19, Features::default()).is_err());
    // A SASL API is refused until SASL is on, and an unserved API always.
    assert!(
        p.check(ApiKey::SaslAuthenticate, 0, Features::default())
            .is_err()
    );
    assert!(p.check(ApiKey::SaslAuthenticate, 0, ALL).is_ok());
    assert!(p.check(ApiKey::ElectLeaders, 0, ALL).is_err());
}

/// An API the table offers at a major and the crate cannot decode at any version of it vanishes from that
/// profile, which is a deviation from what a real broker of that major lists. Each case is on this list on purpose: a new one
/// has to be added here, with its reason.
#[test]
fn the_apis_dropped_because_the_crate_cannot_decode_them_are_known() {
    let mut dropped = Vec::new();
    for major in KafkaMajor::RANGE {
        for row in API_TABLE {
            let offered = row.offered.get(usize::from(major - 1)).copied().flatten();
            if offered.is_some() && profile(major).range(row.key).is_none() {
                dropped.push((major, row.key));
            }
        }
    }
    // None: Kafka 1.x has only v0 of the three ACL APIs, of OffsetForLeaderEpoch and of DescribeLogDirs, which KIP-896
    // removed from the crate's decoders; the vendored crate reads them again (PATCHES.md 5), so nothing is dropped.
    assert_eq!(dropped, Vec::<(u8, ApiKey)>::new());
}

#[test]
fn only_the_served_apis_are_advertised_and_accepted() {
    let features = Features {
        served: ApiSet::of(&[ApiKey::ApiVersions, ApiKey::Metadata]),
        ..Features::default()
    };
    let keys: Vec<_> = profile(4)
        .advertised(features)
        .iter()
        .map(|a| a.key)
        .collect();
    assert_eq!(keys, [ApiKey::Metadata, ApiKey::ApiVersions]);
    assert!(profile(4).check(ApiKey::Metadata, 12, features).is_ok());
    // Offered by the profile, but not served: refused as if never offered.
    assert!(profile(4).check(ApiKey::Fetch, 12, features).is_err());
}

#[test]
fn the_two_sasl_apis_need_both_the_feature_and_a_handler() {
    let sasl = [ApiKey::SaslHandshake, ApiKey::SaslAuthenticate];
    let has = |f: Features, k| profile(4).advertised(f).iter().any(|a| a.key == k);
    let narrowed = ApiSet::of(&[ApiKey::Metadata]);
    let served = narrowed
        .with(ApiKey::SaslHandshake)
        .with(ApiKey::SaslAuthenticate);
    assert!(served.contains(ApiKey::Metadata) && served.contains(ApiKey::SaslHandshake));
    assert!(
        !narrowed.contains(ApiKey::SaslAuthenticate),
        "`with` leaves the original alone"
    );
    for key in sasl {
        // Handler but no feature, feature but no handler: neither lists it.
        assert!(!has(
            Features {
                sasl: false,
                served,
                ..Features::default()
            },
            key
        ));
        assert!(!has(
            Features {
                sasl: true,
                served: narrowed,
                ..Features::default()
            },
            key
        ));
        assert!(has(
            Features {
                sasl: true,
                served,
                ..Features::default()
            },
            key
        ));
        let on = Features {
            sasl: true,
            served,
            ..Features::default()
        };
        assert!(profile(4).check(key, 1, on).is_ok());
    }
}

#[test]
fn the_scram_credential_apis_are_offered_from_kafka_2_at_version_0_and_not_at_1() {
    // The recorded brokers (tests/data/api-versions.json): cp-kafka 6.2 (Kafka 2.8), 3.9.1 and 4.3.1 list both at
    // 0..0; the 1.x broker lists neither, so `--kafka-version 1` does not offer them.
    for key in [
        ApiKey::DescribeUserScramCredentials,
        ApiKey::AlterUserScramCredentials,
    ] {
        assert_eq!(profile(1).range(key), None, "{key:?} at 1");
        for major in 2..=4 {
            assert_eq!(
                profile(major).range(key),
                Some((0, 0)),
                "{key:?} at {major}"
            );
        }
    }
}

#[test]
fn log_dirs_leader_epochs_and_client_quotas_are_offered_as_each_major_did_within_what_the_crate_reads()
 {
    // The recorded brokers (tests/data/api-versions.json): Kafka 1.1 lists OffsetForLeaderEpoch and DescribeLogDirs at v0
    // only and neither quota API; 2.8 lists 0..4, 0..2 and 0..1, 0..1; 3.9 lists 0..4, 0..4, 0..1, 0..1; 4.3 lists 2..4,
    // 1..5, 0..1, 0..1. The crate reads OffsetForLeaderEpoch from v0 (v0 and v1 by PATCHES.md 5) and DescribeLogDirs from
    // v0 (PATCHES.md 5) to v5 (PATCHES.md 6).
    let at = |major, key| profile(major).range(key);
    assert_eq!(at(1, ApiKey::OffsetForLeaderEpoch), Some((0, 0)));
    assert_eq!(at(1, ApiKey::DescribeLogDirs), Some((0, 0)));
    assert_eq!(at(1, ApiKey::DescribeClientQuotas), None);
    assert_eq!(at(1, ApiKey::AlterClientQuotas), None);
    assert_eq!(at(2, ApiKey::OffsetForLeaderEpoch), Some((0, 4)));
    assert_eq!(at(2, ApiKey::DescribeLogDirs), Some((0, 2)));
    assert_eq!(at(3, ApiKey::DescribeLogDirs), Some((0, 4)));
    assert_eq!(at(4, ApiKey::OffsetForLeaderEpoch), Some((2, 4)));
    assert_eq!(at(4, ApiKey::DescribeLogDirs), Some((1, 5)));
    for major in 2..=4 {
        assert_eq!(at(major, ApiKey::DescribeClientQuotas), Some((0, 1)));
        assert_eq!(at(major, ApiKey::AlterClientQuotas), Some((0, 1)));
    }
}
