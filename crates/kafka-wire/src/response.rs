//! Writing a response: a length, the response header, then the body.

use bytes::{Bytes, BytesMut};
use kafka_protocol::messages::ResponseHeader;
use kafka_protocol::protocol::Encodable;

use crate::api::Response;
use crate::error::WireError;

/// The whole response frame for `response`, written at `version`, answering the request with
/// `correlation_id`.
///
/// The header version follows the API and version (flexible versions add an empty tagged-fields block),
/// except that ApiVersions always answers in the v0 header: a client that does not yet know what versions
/// exist cannot know the header's form.
///
/// # Errors
/// A response that cannot be written at that version, or one larger than a frame can hold.
pub fn encode(correlation_id: i32, version: i16, response: &Response) -> Result<Bytes, WireError> {
    let mut out = BytesMut::from(&[0u8; 4][..]);
    let mut header = ResponseHeader::default();
    header.correlation_id = correlation_id;
    let header_version = response.api_key().response_header_version(version);
    header
        .encode(&mut out, header_version)
        .map_err(|e| WireError::encode("response header", e))?;
    response.encode(version, &mut out)?;
    // The length on the wire is a signed 32-bit number.
    let len = i32::try_from(out.len() - 4)
        .map_err(|_| WireError::encode("response", "longer than a frame can be"))?;
    if let Some(prefix) = out.first_chunk_mut::<4>() {
        *prefix = len.to_be_bytes();
    }
    Ok(out.freeze())
}

#[cfg(test)]
mod tests;
