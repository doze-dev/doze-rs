use bytes::Bytes;
use kafka_protocol::messages::ApiKey;

use super::*;

#[test]
fn every_row_names_a_distinct_api() {
    let mut keys: Vec<i16> = API_TABLE.iter().map(|r| r.key as i16).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), API_TABLE.len());
    assert_eq!(API_TABLE.len(), 41);
}

#[test]
fn an_unserved_key_is_refused() {
    let err = Request::decode(ApiKey::OffsetForLeaderEpoch, 0, &mut Bytes::new()).unwrap_err();
    assert_eq!(err, WireError::NotServed(ApiKey::OffsetForLeaderEpoch));
}

#[test]
fn a_request_round_trips_through_its_key() {
    let mut body = BytesMut::new();
    let req = ApiVersionsRequest::default();
    Encodable::encode(&req, &mut body, 3).unwrap();
    let got = Request::decode(ApiKey::ApiVersions, 3, &mut body.freeze()).unwrap();
    assert_eq!(got.api_key(), ApiKey::ApiVersions);
}

#[test]
fn a_response_encodes_at_its_version() {
    let mut out = BytesMut::new();
    Response::ApiVersions(ApiVersionsResponse::default())
        .encode(0, &mut out)
        .unwrap();
    assert!(!out.is_empty());
}
