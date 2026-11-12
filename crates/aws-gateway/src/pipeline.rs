//! One request, from the socket to the answer.

use std::collections::HashMap;

use aws_auth::claim::Claim;
use aws_auth::{AuthError, Endpoint, begin};
use aws_schema::arena::{Arena, NIL};
use aws_schema::enc::encode;
use aws_schema::json::decode;
use aws_schema::meta::Protocol;
use aws_schema::patterns::Patterns;
use aws_wire::service::{Call, Caller, Service};
use aws_wire::{ApiError, Family, RequestId, from_auth, json};
use bytes::Bytes;
use doze_runtime::{Clock, SystemClock};
use http::Response;
use http::request::Parts;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;

use crate::{Auth, Config};

/// A registered service and the pattern cache for its model.
pub(crate) struct Entry {
    pub(crate) service: std::sync::Arc<dyn Service>,
    pub(crate) patterns: Patterns,
}

/// What every request handler shares.
pub(crate) struct Shared {
    entries: Vec<Entry>,
    by_signing_name: HashMap<&'static str, usize>,
    config: Config,
}

impl Shared {
    pub(crate) fn new(entries: Vec<Entry>, config: Config) -> Self {
        let by_signing_name = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.service.meta().signing_name, i))
            .collect();
        Self {
            entries,
            by_signing_name,
            config,
        }
    }

    fn entry(&self, signing_name: &str) -> Option<&Entry> {
        self.by_signing_name
            .get(signing_name)
            .and_then(|&i| self.entries.get(i))
    }
}

type Out = Response<Vec<u8>>;
/// An error answer, boxed so the `Err` of a `Result` stays small.
type Fail = Box<Out>;

/// Answers a request. Never fails: a failure is an error answer.
pub(crate) async fn respond(
    shared: &Shared,
    req: http::Request<Incoming>,
) -> Response<Full<Bytes>> {
    let id = RequestId::new();
    let (parts, body) = req.into_parts();
    let out = match handle(shared, &id, &parts, body).await {
        Ok(out) => out,
        Err(out) => *out,
    };
    out.map(|b| Full::new(Bytes::from(b)))
}

async fn handle(
    shared: &Shared,
    id: &RequestId,
    parts: &Parts,
    body: Incoming,
) -> Result<Out, Fail> {
    let claim = Claim::parse(parts)
        .map_err(|e| early(id, &from_auth(&e, Family::Coral)))?
        .ok_or_else(|| early(id, &from_auth(&AuthError::Missing, Family::Coral)))?;
    let entry = shared.entry(&claim.service).ok_or_else(|| {
        early(
            id,
            &ApiError::new(
                400,
                "UnrecognizedClientException",
                format!("No service is signed for as '{}'.", claim.service),
            ),
        )
    })?;
    let (meta, tables) = (entry.service.meta(), entry.service.tables());
    let Some(version) = json::Version::of(meta) else {
        let wire = if meta.speaks(Protocol::AwsQuery) {
            "awsQuery"
        } else {
            "a REST or CBOR protocol"
        };
        return Err(early(
            id,
            &ApiError::new(
                501,
                "NotImplemented",
                format!(
                    "{} speaks {wire}, which this build does not serve yet.",
                    meta.sdk_id
                ),
            ),
        ));
    };
    let fail = |e: &ApiError| -> Fail { Box::new(json::failure(meta, tables, version, id, e)) };
    let authed = authenticate(shared, parts, claim.service.as_str())
        .map_err(|e| fail(&from_auth(&e, Family::Coral)))?;
    let bytes = read_body(body, shared.config.max_body_bytes)
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
    let op = json::route(meta, tables, parts).map_err(|e| fail(&e))?;
    let caller = Caller {
        access_key_id: claim.access_key_id,
        account_id: shared.config.account_id.clone(),
        region: claim.region,
        endpoint: endpoint_of(parts),
    };
    let call = decode_call(entry, op, id, caller, bytes).map_err(|e| fail(&e))?;
    let output = entry.service.call(call).await.map_err(|e| fail(&e))?;
    let encoded = encode_output(entry, op, &output.arena, output.root).map_err(|e| fail(&e))?;
    Ok(json::success(version, id, encoded))
}

/// The scheme and host the caller used, for URLs in answers.
fn endpoint_of(parts: &Parts) -> String {
    let host = parts
        .headers
        .get(http::header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost");
    format!("http://{host}")
}

/// An error raised before a service is known.
fn early(id: &RequestId, err: &ApiError) -> Fail {
    Box::new(json::generic_failure(json::Version::V10, id, err))
}

/// Checks the head of the request in `Verify` mode; `None` in `Trust` mode.
fn authenticate(
    shared: &Shared,
    parts: &Parts,
    signing_name: &str,
) -> Result<Option<aws_auth::Pending>, AuthError> {
    let Auth::Verify(creds) = &shared.config.auth else {
        return Ok(None);
    };
    let Some(entry) = shared.entry(signing_name) else {
        return Err(AuthError::Missing);
    };
    let endpoint = Endpoint {
        signing_name: entry.service.meta().signing_name,
        s3: false,
        max_skew_secs: shared.config.max_skew_secs,
    };
    let now = SystemClock.now_ms() / 1000;
    begin(parts, now, endpoint, creds.as_ref()).map(Some)
}

async fn read_body(body: Incoming, limit: usize) -> Result<Bytes, ApiError> {
    match Limited::new(body, limit).collect().await {
        Ok(collected) => Ok(collected.to_bytes()),
        Err(e) if e.is::<http_body_util::LengthLimitError>() => Err(ApiError::new(
            413,
            "RequestEntityTooLarge",
            format!("The request body exceeds {limit} bytes."),
        )),
        Err(e) => Err(ApiError::new(
            400,
            "IncompleteBody",
            format!("The request body could not be read: {e}"),
        )),
    }
}

/// Decodes and validates the body against the operation's input shape.
fn decode_call(
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
            decode(tables, &entry.patterns, shape, text, &mut arena)
                .map_err(|v| entry.service.invalid(&v))?
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

fn encode_output(entry: &Entry, op: u32, arena: &Arena, root: u32) -> Result<Vec<u8>, ApiError> {
    let tables = entry.service.tables();
    let Some(shape) = tables.op(op).and_then(|o| o.output) else {
        return Ok(b"{}".to_vec());
    };
    let mut out = Vec::new();
    encode(tables, shape, arena, "", root, &mut out)
        .map_err(|_| ApiError::internal("The response could not be written."))?;
    Ok(out)
}
