//! A JSON request's body as the service's input, and the service's output as a JSON body.

use aws_schema::arena::{Arena, NIL};
use aws_schema::enc::encode;
use aws_schema::json::decode;
use aws_wire::service::{Call, Caller};
use aws_wire::{ApiError, RequestId};
use bytes::Bytes;

use super::Entry;

/// Decodes and validates the body against the operation's input shape.
pub(crate) fn decode_call(
    entry: &Entry,
    op: u32,
    id: &RequestId,
    caller: Caller,
    bytes: Bytes,
) -> Result<Call, ApiError> {
    let tables = entry.service.tables();
    let body = body_text(bytes).map_err(|_| {
        ApiError::new(
            400,
            "SerializationException",
            "The request body is not valid UTF-8.",
        )
    })?;
    let input = tables.op(op).and_then(|o| o.input);
    let mut arena = Arena::new();
    let root = match input {
        Some(shape) => {
            let text = if body.trim().is_empty() {
                "{}"
            } else {
                body.as_str()
            };
            decode(tables, &entry.patterns, shape, text, &mut arena).map_err(|v| {
                let e = entry.service.invalid_in(&v, text);
                aws_wire::located::place(e, tables, &entry.patterns, shape, text, &v)
            })?
        }
        None => NIL,
    };
    Ok(Call {
        op,
        id: id.clone(),
        caller,
        body,
        arena,
        root,
    })
}

/// The body as text, checked for UTF-8. A body the gateway holds alone (one read in several chunks, or inflated) becomes the text
/// without a copy; only one that shares its buffer with hyper's is copied, once.
pub(crate) fn body_text(bytes: Bytes) -> Result<String, std::string::FromUtf8Error> {
    String::from_utf8(Vec::from(bytes))
}

pub(super) fn encode_output(
    entry: &Entry,
    op: u32,
    arena: &Arena,
    root: u32,
) -> Result<Vec<u8>, ApiError> {
    let tables = entry.service.tables();
    let Some(shape) = tables.op(op).and_then(|o| o.output) else {
        return Ok(b"{}".to_vec());
    };
    // An operation whose output is an empty structure answers with no tree at all.
    if root == NIL {
        return Ok(b"{}".to_vec());
    }
    // What the tree holds is about what it writes: one allocation, not a doubling for each size a large answer passes.
    let mut out = Vec::with_capacity(arena.held_bytes().min(32 << 20));
    encode(tables, shape, arena, "", root, &mut out)
        .map_err(|_| ApiError::internal("The response could not be written."))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn a_body_held_alone_becomes_text_without_a_copy() {
        let held = br#"{"QueueUrl":"q"}"#.to_vec();
        let at = held.as_ptr();
        let text = body_text(Bytes::from(held)).unwrap();
        assert_eq!(text, r#"{"QueueUrl":"q"}"#);
        assert_eq!(text.as_ptr(), at, "the body's own buffer is the text");
        assert!(body_text(Bytes::from_static(b"\xff")).is_err());
    }
}
