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

/// What Kafka 4.4.0 answers (KIP-890 oracle, `ApiVersions` v4 and v3): `transaction.version` supported 0 to 2 and
/// finalized at 2, the finalized epoch not negative; at v3 the supported row (least level 0) is left out and the finalized
/// one kept. A profile of the classic protocol (Kafka 1 to 3) lists no feature, and nothing is listed below v3.
#[test]
fn kafka_4_finalizes_transaction_version_2_and_the_older_majors_list_no_feature() {
    let ask = |major: u8, version: i16| {
        answer(
            Profile::new(KafkaMajor::new(major).unwrap()),
            Features::default(),
            version,
            None,
        )
        .1
    };
    let names = |r: &ApiVersionsResponse| {
        (
            r.supported_features
                .iter()
                .map(|f| (f.name.to_string(), f.min_version, f.max_version))
                .collect::<Vec<_>>(),
            r.finalized_features
                .iter()
                .map(|f| (f.name.to_string(), f.min_version_level, f.max_version_level))
                .collect::<Vec<_>>(),
        )
    };
    let tv = || "transaction.version".to_owned();
    let v4 = ask(4, 4);
    assert_eq!(names(&v4), (vec![(tv(), 0, 2)], vec![(tv(), 2, 2)]));
    assert!(v4.finalized_features_epoch >= 0);
    let v3 = ask(4, 3);
    assert_eq!(names(&v3), (vec![], vec![(tv(), 2, 2)]));
    assert!(v3.finalized_features_epoch >= 0);
    for version in 0..=2 {
        assert_eq!(names(&ask(4, version)), (vec![], vec![]), "v{version}");
    }
    for major in 1..=3 {
        let r = ask(major, 3);
        assert_eq!(names(&r), (vec![], vec![]), "Kafka {major}");
        assert_eq!(r.finalized_features_epoch, -1, "Kafka {major}");
    }
}
