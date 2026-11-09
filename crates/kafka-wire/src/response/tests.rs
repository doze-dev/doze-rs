//! Each response is read back with the crate's own decoders, at the header version the protocol fixes
//! for it, and must use every byte: a header of the wrong version shifts the body and shows up here.

use kafka_protocol::messages::{
    ApiKey, ApiVersionsResponse, ResponseHeader, api_versions_response::ApiVersion,
};
use kafka_protocol::protocol::Decodable;

use super::*;

/// The frame's length prefix, checked against the bytes after it, and the payload.
fn payload(frame: &Bytes) -> Bytes {
    let (prefix, rest) = frame.split_at(4);
    let declared = i32::from_be_bytes(<[u8; 4]>::try_from(prefix).unwrap());
    assert_eq!(usize::try_from(declared).unwrap(), rest.len());
    Bytes::copy_from_slice(rest)
}

fn api_versions_body() -> ApiVersionsResponse {
    let mut row = ApiVersion::default();
    (row.api_key, row.min_version, row.max_version) = (18, 0, 4);
    let mut response = ApiVersionsResponse::default();
    response.api_keys.push(row);
    response
}

#[test]
fn api_versions_uses_the_v0_response_header_at_every_version() {
    // ApiVersions v3 and v4 are flexible, but the header stays v0: no tagged-fields byte.
    for version in 0..=4 {
        assert_eq!(ApiKey::ApiVersions.response_header_version(version), 0);
        let frame = encode(7, version, &Response::ApiVersions(api_versions_body())).unwrap();
        let mut rest = payload(&frame);
        let header = ResponseHeader::decode(&mut rest, 0).unwrap();
        assert_eq!(header.correlation_id, 7);
        let body = ApiVersionsResponse::decode(&mut rest, version).unwrap();
        assert_eq!(body.api_keys.len(), 1, "v{version}");
        assert!(
            rest.is_empty(),
            "v{version}: {} bytes left over",
            rest.len()
        );
    }
}
