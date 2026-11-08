//! Helpers for tests, here and in other crates (feature `test-support`).
//!
//! These exist to fail loudly when a test builds something that cannot be encoded, so they may `expect`.
#![allow(clippy::expect_used)]

use bytes::{BufMut, Bytes, BytesMut};
use kafka_protocol::messages::{ApiKey, RequestHeader};
use kafka_protocol::protocol::{Encodable, StrBytes};

/// A request frame's payload the way a client writes it: header, then `body` encoded at `version`.
///
/// # Panics
/// If the header or body cannot be encoded, which a test would rather see loudly.
#[must_use]
pub fn request_payload<R: Encodable>(
    key: ApiKey,
    version: i16,
    correlation_id: i32,
    client_id: Option<&str>,
    body: &R,
) -> Bytes {
    let mut out = BytesMut::new();
    let mut h = RequestHeader::default();
    h.request_api_key = key as i16;
    h.request_api_version = version;
    h.correlation_id = correlation_id;
    h.client_id = client_id.map(|c| StrBytes::from_string(c.to_owned()));
    h.encode(&mut out, key.request_header_version(version))
        .expect("encodes a header");
    body.encode(&mut out, version).expect("encodes a body");
    out.freeze()
}

/// `payload` with its 4-byte big-endian length in front, as it is on the wire.
#[must_use]
pub fn framed(payload: &[u8]) -> Bytes {
    let mut out = BytesMut::with_capacity(payload.len() + 4);
    out.put_u32(u32::try_from(payload.len()).expect("a test frame fits"));
    out.put_slice(payload);
    out.freeze()
}
