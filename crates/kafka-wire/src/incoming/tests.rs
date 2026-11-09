use kafka_protocol::messages::{ApiVersionsRequest, FetchRequest};

use super::*;
use crate::profile::KafkaMajor;
use crate::testing::request_payload;

fn profile() -> Profile {
    Profile::new(KafkaMajor::DEFAULT)
}

#[test]
fn a_version_outside_the_range_is_refused() {
    let p = request_payload(ApiKey::Fetch, 14, 1, None, &FetchRequest::default());
    assert!(matches!(
        read(p, profile(), Features::default()),
        Err(WireError::NotAdvertised {
            key: ApiKey::Fetch,
            version: 14
        })
    ));
}

#[test]
fn api_versions_with_an_unreadable_body_is_still_answered() {
    let p = request_payload(
        ApiKey::ApiVersions,
        3,
        1,
        None,
        &ApiVersionsRequest::default(),
    );
    let cut = p.slice(..p.len() - 1);
    assert!(matches!(
        read(cut, profile(), Features::default()),
        Ok(Incoming::ApiVersions { body: None, .. })
    ));
}

#[test]
fn the_body_of_an_unserved_api_versions_version_is_not_read() {
    // Kafka 1.x serves ApiVersions v0 to v1; a v3 body is perfectly readable, and is still left alone.
    let major_one = Profile::new(KafkaMajor::new(1).unwrap());
    assert_eq!(major_one.range(ApiKey::ApiVersions), Some((0, 1)));
    let p = request_payload(
        ApiKey::ApiVersions,
        3,
        1,
        None,
        &ApiVersionsRequest::default(),
    );
    assert!(matches!(
        read(p.clone(), major_one, Features::default()),
        Ok(Incoming::ApiVersions { body: None, .. })
    ));
    // The same bytes at a major that serves v3 are read.
    assert!(matches!(
        read(p, profile(), Features::default()),
        Ok(Incoming::ApiVersions { body: Some(_), .. })
    ));
}
