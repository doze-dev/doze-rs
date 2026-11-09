use kafka_protocol::messages::ApiKey;

use super::*;

fn profile(major: u8) -> Profile {
    Profile::new(KafkaMajor::new(major).unwrap())
}

const ALL: Features = Features {
    sasl: true,
    telemetry: true,
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

#[test]
fn each_major_offers_what_it_should() {
    assert!(!profile(1).offers_consumer_groups() && !profile(3).offers_consumer_groups());
    assert!(profile(4).offers_consumer_groups());
    assert_eq!(profile(1).range(ApiKey::DescribeCluster), None);
    assert!(profile(3).range(ApiKey::DescribeCluster).is_some());
    assert_eq!(profile(4).range(ApiKey::Fetch), Some((4, 13)));
    assert_eq!(profile(1).range(ApiKey::Produce), Some((3, 5)));
}

#[test]
fn outside_the_range_is_not_served() {
    let p = profile(4);
    assert!(p.check(ApiKey::Fetch, 13, Features::default()).is_ok());
    assert!(p.check(ApiKey::Fetch, 3, Features::default()).is_err());
    assert!(p.check(ApiKey::Fetch, 14, Features::default()).is_err());
    // A SASL API is refused until SASL is on, and an unserved API always.
    assert!(
        p.check(ApiKey::SaslAuthenticate, 0, Features::default())
            .is_err()
    );
    assert!(p.check(ApiKey::SaslAuthenticate, 0, ALL).is_ok());
    assert!(p.check(ApiKey::OffsetForLeaderEpoch, 0, ALL).is_err());
}

/// An API the table offers at a major and the crate cannot decode at any version of it vanishes from that
/// profile, which is a deviation from what a real broker of that major lists. There is one such case,
/// and it is on this list on purpose: a new one has to be added here, with its reason.
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
    // Kafka 1.x has only v0 of the three ACL APIs, and KIP-896 removed v0 from the crate's decoders.
    assert_eq!(
        dropped,
        vec![
            (1, ApiKey::DescribeAcls),
            (1, ApiKey::CreateAcls),
            (1, ApiKey::DeleteAcls)
        ]
    );
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
