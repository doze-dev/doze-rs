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
