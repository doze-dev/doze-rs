use super::*;
use crate::profile::KafkaMajor;

#[test]
fn an_unreadable_body_at_a_served_version_gets_the_list() {
    let (version, resp) = answer(
        Profile::new(KafkaMajor::DEFAULT),
        Features::default(),
        4,
        None,
    );
    assert_eq!((version, resp.error_code), (4, 0));
    assert!(!resp.api_keys.is_empty());
}

#[test]
fn the_answer_to_an_apiversions_in_the_middle_of_a_login_is_illegal_sasl_state_with_no_apis() {
    // Recorded (sasl-plain `versions-before-token`): error 34 and an empty list, at the request's own version.
    let profile = Profile::new(KafkaMajor::DEFAULT);
    for version in 0..=4 {
        let (at, resp) = illegal_state(profile, version);
        assert_eq!((at, resp.error_code), (version, 34), "v{version}");
        assert!(resp.api_keys.is_empty(), "v{version}");
    }
    // A version that is not served is answered in the v0 form, as every ApiVersions error is.
    let (at, resp) = illegal_state(profile, 99);
    assert_eq!((at, resp.error_code), (0, 34));
}

/// What Kafka 4.4.0 and 3.9.1 answer (`kafka-features.sh describe`, and `oracle_admin` against 4.4.0, 2026-11-19): every
/// feature the recorded broker supports with its levels, and each one above level 0 finalized, the finalized epoch not
/// negative; at v3 the supported rows whose least level is 0 are left out and the finalized ones kept. Kafka 1 and 2 list no
/// feature, and nothing is listed below v3. A broker that does not serve the share or streams APIs lists neither feature.
#[test]
fn each_major_lists_the_features_of_its_recorded_broker() {
    let ask = |major: u8, version: i16| {
        answer(
            Profile::new(KafkaMajor::new(major).unwrap()),
            Features::default(),
            version,
            None,
        )
        .1
    };
    type Rows = Vec<(String, i16, i16)>;
    let names = |r: &ApiVersionsResponse| -> (Rows, Rows) {
        (
            r.supported_features
                .iter()
                .map(|f| (f.name.to_string(), f.min_version, f.max_version))
                .collect(),
            r.finalized_features
                .iter()
                .map(|f| (f.name.to_string(), f.min_version_level, f.max_version_level))
                .collect(),
        )
    };
    let rows = |r: &[(&str, i16, i16)]| -> Rows {
        r.iter().map(|&(n, a, b)| (n.to_owned(), a, b)).collect()
    };
    let v4 = ask(4, 4);
    assert_eq!(
        names(&v4),
        (
            rows(&[
                ("eligible.leader.replicas.version", 0, 1),
                ("group.version", 0, 1),
                ("kraft.version", 0, 1),
                ("metadata.version", 7, 33),
                ("share.version", 0, 2),
                ("streams.version", 0, 1),
                ("transaction.version", 0, 2),
            ]),
            rows(&[
                ("eligible.leader.replicas.version", 1, 1),
                ("group.version", 1, 1),
                ("metadata.version", 33, 33),
                ("share.version", 2, 2),
                ("streams.version", 1, 1),
                ("transaction.version", 2, 2),
            ])
        )
    );
    assert!(v4.finalized_features_epoch >= 0);
    let v3 = ask(4, 3);
    assert_eq!(names(&v3).0, rows(&[("metadata.version", 7, 33)]));
    assert_eq!(names(&v3).1, names(&v4).1);
    let no_groups = Features {
        served: crate::profile::ApiSet::of(&[ApiKey::ApiVersions]),
        ..Features::default()
    };
    let without = answer(
        Profile::new(KafkaMajor::new(4).unwrap()),
        no_groups,
        4,
        None,
    )
    .1;
    assert!(
        names(&without)
            .0
            .iter()
            .all(|(n, _, _)| n != "share.version" && n != "streams.version")
    );
    for version in 0..=2 {
        assert_eq!(names(&ask(4, version)), (vec![], vec![]), "v{version}");
    }
    assert_eq!(
        names(&ask(3, 3)),
        (
            rows(&[("metadata.version", 1, 21)]),
            rows(&[("metadata.version", 21, 21)])
        )
    );
    for major in 1..=2 {
        let r = ask(major, 3);
        assert_eq!(names(&r), (vec![], vec![]), "Kafka {major}");
        assert_eq!(r.finalized_features_epoch, -1, "Kafka {major}");
    }
}
