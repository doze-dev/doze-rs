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
    let body = String::from_utf8(bytes.to_vec()).map_err(|_| {
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
    let mut out = Vec::new();
    encode(tables, shape, arena, "", root, &mut out)
        .map_err(|_| ApiError::internal("The response could not be written."))?;
    Ok(out)
}
