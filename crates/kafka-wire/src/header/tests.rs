use bytes::{BufMut, BytesMut};

use super::*;

/// A header the way a client writes it: key, version, correlation id, client id, tagged fields if flexible.
fn header(key: i16, version: i16, flexible: bool, client: Option<&str>) -> BytesMut {
    let mut b = BytesMut::new();
    b.put_i16(key);
    b.put_i16(version);
    b.put_i32(0x0102_0304);
    match client {
        Some(c) => {
            b.put_i16(c.len() as i16);
            b.put_slice(c.as_bytes());
        }
        None => b.put_i16(-1),
    }
    if flexible {
        b.put_u8(0); // an empty tagged-fields block
    }
    b
}

#[test]
fn parses_a_plain_header() {
    // Metadata v4 is not flexible.
    let mut b = header(3, 4, false, Some("rdkafka"));
    b.put_slice(b"body");
    let (head, body) = split(b.freeze()).unwrap();
    assert_eq!(head.key, ApiKey::Metadata);
    assert_eq!((head.version, head.correlation_id), (4, 0x0102_0304));
    assert_eq!(head.client_id.as_deref(), Some("rdkafka"));
    assert_eq!(&body[..], b"body");
}

#[test]
fn parses_a_flexible_header() {
    // Metadata v12 is flexible: tagged fields follow the client id and are not part of the body.
    let mut b = header(3, 12, true, Some("c"));
    b.put_slice(b"body");
    let (head, body) = split(b.freeze()).unwrap();
    assert_eq!(head.version, 12);
    assert_eq!(&body[..], b"body");
}

#[test]
fn a_null_client_id_is_none() {
    let (head, _) = split(header(3, 4, false, None).freeze()).unwrap();
    assert_eq!(head.client_id, None);
}

#[test]
fn an_unknown_key_is_an_error() {
    // 4 (LeaderAndIsr) is an inter-broker API this broker has no name for.
    let err = split(header(4, 0, false, None).freeze()).unwrap_err();
    assert_eq!(err, WireError::UnknownApi(4));
    assert_eq!(
        split(header(999, 0, false, None).freeze()).unwrap_err(),
        WireError::UnknownApi(999)
    );
}

#[test]
fn a_short_header_is_an_error() {
    for n in 0..4 {
        assert!(split(Bytes::from(vec![0u8; n])).is_err());
    }
    let mut cut = header(3, 4, false, Some("client"));
    cut.truncate(10);
    assert!(split(cut.freeze()).is_err());
}
