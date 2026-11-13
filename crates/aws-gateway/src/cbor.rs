//! A request in the Smithy RPC v2 CBOR protocol: the same steps as a JSON one (authenticate, read, route, decode and check against
//! the model, call, answer), with the operation named by the path and the bodies in CBOR. The body is read into the JSON text the
//! JSON decoder checks, so a service sees the same call whichever protocol came in.

use aws_auth::claim::Claim;
use aws_schema::arena::NIL;
use aws_wire::{ApiError, Family, RequestId, cbor, from_auth};
use bytes::Bytes;
use http::request::Parts;
use hyper::body::Incoming;

use crate::pipeline::{
    Entry, Fail, Out, Shared, authenticate, caller_of, decode_call, inflate, read_body, unbuilt,
};

/// Answers a CBOR request to `entry`.
pub(crate) async fn handle(
    shared: &Shared,
    entry: &Entry,
    id: &RequestId,
    parts: &Parts,
    body: Incoming,
    claim: Claim,
) -> Result<Out, Fail> {
    let (meta, tables) = (entry.service.meta(), entry.service.tables());
    let fail = |e: &ApiError| -> Fail {
        Box::new(cbor::failure(
            meta,
            tables,
            id,
            e,
            entry.service.flat_client_errors(),
        ))
    };
    let authed = authenticate(shared, parts, claim.service.as_str())
        .await
        .map_err(|e| fail(&from_auth(&e, Family::Coral)))?;
    let bytes = read_body(body, shared.max_body_bytes())
        .await
        .map_err(|e| fail(&e))?;
    if let Some(pending) = authed {
        let hash = pending
            .needs_body_hash()
            .then(|| aws_auth::key::sha256_hex(&bytes));
        pending
            .finish(parts, hash.as_deref())
            .map_err(|e| fail(&from_auth(&e, Family::Coral)))?;
    }
    let op = cbor::route(meta, tables, parts).map_err(|e| fail(&e))?;
    if let Some(refusal) = unbuilt(entry, op) {
        return Err(fail(&refusal));
    }
    let bytes = inflate(parts, bytes, shared.max_body_bytes()).map_err(|e| fail(&e))?;
    let json = cbor::to_json(&bytes).map_err(|e| {
        fail(&ApiError::new(
            400,
            "SerializationException",
            format!("The request body could not be read: {e}"),
        ))
    })?;
    let caller = caller_of(shared, parts, claim);
    let call = decode_call(entry, op, id, caller, Bytes::from(json)).map_err(|e| fail(&e))?;
    let output = entry.service.call(call).await.map_err(|e| fail(&e))?;
    let encoded = encode_output(entry, op, &output).map_err(|e| fail(&e))?;
    Ok(cbor::success(id, encoded))
}

/// The answer tree as CBOR: an operation with no output (or an empty one) answers an empty map.
fn encode_output(
    entry: &Entry,
    op: u32,
    output: &aws_wire::service::Output,
) -> Result<Vec<u8>, ApiError> {
    let tables = entry.service.tables();
    let mut out = Vec::new();
    match tables
        .op(op)
        .and_then(|o| o.output)
        .filter(|_| output.root != NIL)
    {
        Some(shape) => {
            aws_schema::cbor::encode(tables, shape, &output.arena, "", output.root, &mut out)
                .map_err(|_| ApiError::internal("The response could not be written."))?
        }
        None => aws_schema::cbor::map_header(&mut out, 0),
    }
    Ok(out)
}
