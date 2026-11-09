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
fn an_unknown_key_is_an_error() {
    // 4 (LeaderAndIsr) is an inter-broker API this broker has no name for.
    let err = split(header(4, 0, false, None).freeze()).unwrap_err();
    assert_eq!(err, WireError::UnknownApi(4));
    assert_eq!(
        split(header(999, 0, false, None).freeze()).unwrap_err(),
        WireError::UnknownApi(999)
    );
}
