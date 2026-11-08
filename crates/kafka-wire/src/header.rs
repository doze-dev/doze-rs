//! The request header: who is asking for what, before the body.

use bytes::Bytes;
use kafka_protocol::messages::{ApiKey, RequestHeader};
use kafka_protocol::protocol::Decodable;

use crate::error::WireError;

/// The fields every request starts with, decoded far enough to route and answer it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestHead {
    /// The API asked for.
    pub key: ApiKey,
    /// The version of it.
    pub version: i16,
    /// What the answer must echo, so a client can match it to its request.
    pub correlation_id: i32,
    /// The client's own name for itself, if it sent one.
    pub client_id: Option<String>,
}

/// Splits one frame's payload into its header and the body after it.
///
/// Which header version applies (flexible versions carry tagged fields after the client id) depends on the
/// API and its version, so the key and version are read first.
///
/// # Errors
/// A payload too short for a header, an API key with no name, or a header that does not decode.
pub fn split(mut frame: Bytes) -> Result<(RequestHead, Bytes), WireError> {
    let (Some(raw_key), Some(version)) = (peek_i16(&frame, 0), peek_i16(&frame, 2)) else {
        return Err(WireError::decode(
            "request header",
            "shorter than an API key and version",
        ));
    };
    let key = ApiKey::try_from(raw_key).map_err(|()| WireError::UnknownApi(raw_key))?;
    let h = RequestHeader::decode(&mut frame, key.request_header_version(version))
        .map_err(|e| WireError::decode("request header", e))?;
    let head = RequestHead {
        key,
        version,
        correlation_id: h.correlation_id,
        client_id: h.client_id.map(|s| s.to_string()),
    };
    Ok((head, frame))
}

fn peek_i16(b: &Bytes, at: usize) -> Option<i16> {
    b.get(at..)
        .and_then(|s| s.first_chunk::<2>())
        .map(|s| i16::from_be_bytes(*s))
}

#[cfg(test)]
mod tests;
