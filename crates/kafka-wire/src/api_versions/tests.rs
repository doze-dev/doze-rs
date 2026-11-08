use kafka_protocol::protocol::StrBytes;

use super::*;
use crate::profile::KafkaMajor;

fn profile() -> Profile {
    Profile::new(KafkaMajor::DEFAULT)
}

fn named(name: &str, version: &str) -> ApiVersionsRequest {
    let mut q = ApiVersionsRequest::default();
    q.client_software_name = StrBytes::from_string(name.to_owned());
    q.client_software_version = StrBytes::from_string(version.to_owned());
    q
}

#[test]
fn software_names() {
    for ok in [
        "a",
        "rdkafka",
        "librdkafka-2.3.0",
        "apache-kafka-java",
        "1.2",
    ] {
        assert!(valid_client_software(ok), "{ok}");
    }
    for bad in ["", "-a", "a-", ".a", "a.", "a b", "a_b", "caf\u{e9}", "a/b"] {
        assert!(!valid_client_software(bad), "{bad:?}");
    }
}

#[test]
fn the_list_is_the_profiles() {
    let (version, resp) = answer(
        profile(),
        Features::default(),
        3,
        Some(&named("rdkafka", "2.3.0")),
    );
    assert_eq!((version, resp.error_code), (3, 0));
    assert_eq!(
        resp.api_keys.len(),
        profile().advertised(Features::default()).len()
    );
}

#[test]
fn an_unserved_version_is_answered_in_v0() {
    for asked in [5, 99, -1] {
        let (version, resp) = answer(profile(), Features::default(), asked, None);
        assert_eq!(version, 0);
        assert_eq!(resp.error_code, ResponseError::UnsupportedVersion.code());
        assert_eq!(resp.api_keys.len(), 1);
        assert_eq!(resp.api_keys[0].api_key, ApiKey::ApiVersions as i16);
        assert_eq!(
            (resp.api_keys[0].min_version, resp.api_keys[0].max_version),
            (0, 4)
        );
    }
}

#[test]
fn bad_client_software_is_invalid_request() {
    let (version, resp) = answer(profile(), Features::default(), 3, Some(&named("", "1.0")));
    assert_eq!(
        (version, resp.error_code),
        (3, ResponseError::InvalidRequest.code())
    );
    assert!(resp.api_keys.is_empty());
    // Before v3 a client says nothing about itself, so nothing is checked.
    let (_, resp) = answer(profile(), Features::default(), 2, Some(&named("", "")));
    assert_eq!(resp.error_code, 0);
}

#[test]
fn an_unreadable_body_at_a_served_version_gets_the_list() {
    let (version, resp) = answer(profile(), Features::default(), 4, None);
    assert_eq!((version, resp.error_code), (4, 0));
    assert!(!resp.api_keys.is_empty());
}
