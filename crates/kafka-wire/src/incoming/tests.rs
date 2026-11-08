use kafka_protocol::messages::{
    ApiVersionsRequest, FetchRequest, MetadataRequest, OffsetForLeaderEpochRequest,
};

use super::*;
use crate::profile::KafkaMajor;
use crate::testing::request_payload;

fn profile() -> Profile {
    Profile::new(KafkaMajor::DEFAULT)
}

#[test]
fn a_served_request_is_decoded() {
    let p = request_payload(
        ApiKey::Metadata,
        12,
        5,
        Some("c"),
        &MetadataRequest::default(),
    );
    let Incoming::Request { head, request } = read(p, profile(), Features::default()).unwrap()
    else {
        panic!("expected a request");
    };
    assert_eq!((head.correlation_id, head.version), (5, 12));
    assert_eq!(request.api_key(), ApiKey::Metadata);
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
fn an_api_that_is_not_served_is_refused() {
    let p = request_payload(
        ApiKey::OffsetForLeaderEpoch,
        4,
        1,
        None,
        &OffsetForLeaderEpochRequest::default(),
    );
    assert!(matches!(
        read(p, profile(), Features::default()),
        Err(WireError::NotAdvertised { .. })
    ));
}

#[test]
fn a_body_that_does_not_decode_is_an_error() {
    let mut p =
        request_payload(ApiKey::Metadata, 12, 1, None, &MetadataRequest::default()).to_vec();
    p.truncate(p.len() - 3);
    assert!(matches!(
        read(Bytes::from(p), profile(), Features::default()),
        Err(WireError::Decode { .. })
    ));
}

#[test]
fn api_versions_is_never_refused_for_its_version() {
    // v9 is not served: the request is still read, so it can be answered in v0.
    let p = request_payload(
        ApiKey::ApiVersions,
        3,
        1,
        None,
        &ApiVersionsRequest::default(),
    );
    let mut raw = p.to_vec();
    raw[3] = 9;
    match read(Bytes::from(raw), profile(), Features::default()).unwrap() {
        Incoming::ApiVersions { head, body } => {
            assert_eq!(head.version, 9);
            assert!(body.is_none());
        }
        other @ Incoming::Request { .. } => panic!("{other:?}"),
    }
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

#[test]
fn the_header_can_be_judged_before_any_body_is_decoded() {
    // A Metadata request whose body is garbage: the header splits fine, so a server that refuses the API
    // for an unauthenticated peer can do so without ever decoding what follows.
    let mut p =
        request_payload(ApiKey::Metadata, 12, 3, None, &MetadataRequest::default()).to_vec();
    let body_at = p.len() - 2;
    p.truncate(body_at);
    p.extend_from_slice(&[0xff; 64]);
    let (head, body) = header::split(Bytes::from(p)).unwrap();
    assert_eq!((head.key, head.correlation_id), (ApiKey::Metadata, 3));
    assert!(Incoming::decode(head, body, profile(), Features::default()).is_err());
}
