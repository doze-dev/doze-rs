//! Writing a response: a length, the response header, then the body.

use bytes::{BufMut, Bytes, BytesMut};
use kafka_protocol::messages::{ApiKey, ResponseHeader};
use kafka_protocol::protocol::Encodable;

use crate::api::Response;
use crate::error::WireError;

mod fetch;
mod frame;

pub use frame::Frame;

/// The whole response frame for `response`, written at `version`, answering the request with
/// `correlation_id`, in one buffer.
///
/// The header version follows the API and version (flexible versions add an empty tagged-fields block),
/// except that ApiVersions always answers in the v0 header: a client that does not yet know what versions
/// exist cannot know the header's form.
///
/// # Errors
/// A response that cannot be written at that version, or one larger than a frame can hold.
pub fn encode(correlation_id: i32, version: i16, response: &Response) -> Result<Bytes, WireError> {
    let (header, header_version) = header(correlation_id, response.api_key(), version);
    // Sized once from what the protocol computes, so a large Fetch is written into one allocation rather than grown
    // (and copied) a doubling at a time. The length is still taken from what was written.
    let size = header
        .compute_size(header_version)
        .map_err(|e| WireError::encode("response header", e))?
        + response.compute_size(version)?;
    let mut out = BytesMut::with_capacity(4 + size);
    out.put_bytes(0, 4);
    header
        .encode(&mut out, header_version)
        .map_err(|e| WireError::encode("response header", e))?;
    response.encode(version, &mut out)?;
    let len = frame_len(out.len() - 4)?;
    if let Some(prefix) = out.first_chunk_mut::<4>() {
        *prefix = len.to_be_bytes();
    }
    Ok(out.freeze())
}

/// The same frame as [`encode`], in the pieces it is to be written in: a Fetch's records stay in the buffers the log
/// was read into and are written to the socket from there (a vectored write), not copied into the response first.
/// Every other response is one piece, written by [`encode`].
///
/// # Errors
/// As [`encode`].
pub fn frame(correlation_id: i32, version: i16, response: &Response) -> Result<Frame, WireError> {
    match response {
        Response::Fetch(fetch) if fetch::VERSIONS.contains(&version) => {
            fetch::frame(correlation_id, version, fetch, fetch::SHARED_FROM)
        }
        _ => encode(correlation_id, version, response).map(Frame::from),
    }
}

/// The response header for `key` at `version`, and the header version it is written at.
fn header(correlation_id: i32, key: ApiKey, version: i16) -> (ResponseHeader, i16) {
    let mut header = ResponseHeader::default();
    header.correlation_id = correlation_id;
    (header, key.response_header_version(version))
}

/// The length prefix of a frame whose payload is `len` bytes: on the wire it is a signed 32-bit number.
fn frame_len(len: usize) -> Result<i32, WireError> {
    i32::try_from(len).map_err(|_| WireError::encode("response", "longer than a frame can be"))
}

#[cfg(test)]
mod tests;
