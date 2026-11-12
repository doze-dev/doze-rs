//! One request, from the socket to the answer.

use std::collections::HashMap;

use crate::body::RespBody;
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
use http_body_util::{BodyExt, Limited};
use hyper::body::Incoming;

use crate::{Auth, Config};

/// A registered service and the pattern cache for its model.
pub(crate) struct Entry {
    pub(crate) service: std::sync::Arc<dyn Service>,
    pub(crate) patterns: Patterns,
    /// The routes of a REST service, built when its first request arrives.
    pub(crate) routes: std::sync::OnceLock<aws_wire::rest::route::Router>,
}

impl Entry {
    pub(crate) fn new(service: std::sync::Arc<dyn Service>) -> Self {
        Self {
            patterns: Patterns::new(service.tables()),
            service,
            routes: std::sync::OnceLock::new(),
        }
    }

    /// The routes of the service's model.
    pub(crate) fn router(
        &self,
        tables: &aws_schema::tables::Tables,
    ) -> &aws_wire::rest::route::Router {
        self.routes
            .get_or_init(|| aws_wire::rest::route::Router::new(tables))
    }
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

    /// The largest request body read.
    pub(crate) fn max_body_bytes(&self) -> usize {
        self.config.max_body_bytes
    }

    /// Whether signatures are taken on trust.
    pub(crate) fn trusts(&self) -> bool {
        matches!(self.config.auth, Auth::Trust)
    }

    /// The account every request is for.
    pub(crate) fn account_id(&self) -> &str {
        &self.config.account_id
    }

    pub(crate) fn entry(&self, signing_name: &str) -> Option<&Entry> {
        self.by_signing_name
            .get(signing_name)
            .and_then(|&i| self.entries.get(i))
    }
}

pub(crate) type Out = Response<Vec<u8>>;
/// An error answer, boxed so the `Err` of a `Result` stays small.
pub(crate) type Fail = Box<Out>;

/// Answers a request. Never fails: a failure is an error answer.
pub(crate) async fn respond(shared: &Shared, req: http::Request<Incoming>) -> Response<RespBody> {
    let id = RequestId::new();
    let (parts, body) = req.into_parts();
    // A REST service (S3) streams its bodies and words its errors its own way, so it has its own path.
    let claim = match Claim::parse(&parts) {
        Ok(found) => found,
        // boto3 signs an S3 presigned URL with SigV2 unless told otherwise, and S3 accepts it. With no keys to check it against, a
        // trusting gateway takes the access key as given and holds the URL to its expiry.
        Err(AuthError::UnsupportedAlgorithm) if shared.trusts() => {
            crate::presign::sigv2_claim(&parts)
        }
        Err(_) => None,
    };
    if let Some(claim) = claim
        && let Some(entry) = shared
            .entry(&claim.service)
            .filter(|e| e.service.meta().speaks(Protocol::RestXml))
    {
        return crate::rest::respond(shared, entry, &id, &parts, body, claim).await;
    }
    let out = match handle(shared, &id, &parts, body).await {
        Ok(out) => out,
        Err(out) => *out,
    };
    out.map(RespBody::whole)
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
        if meta.speaks(Protocol::AwsQuery) {
            return crate::query::handle(shared, entry, id, parts, body, claim).await;
        }
        return Err(early(id, &unserved_protocol(meta)));
    };
    let fail = |e: &ApiError| -> Fail {
        Box::new(json::failure(
            meta,
            tables,
            version,
            id,
            e,
            entry.service.flat_client_errors(),
        ))
    };
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
    if json::is_query_request(parts) {
        return Err(Box::new(json::query_refusal(meta, id)));
    }
    let op = json::route(meta, tables, parts).map_err(|e| fail(&e))?;
    if !entry.service.serves(op) {
        let name = tables.op(op).map_or("", |o| o.name);
        return Err(fail(&ApiError::new(
            501,
            "NotImplemented",
            format!("{name} is not built in doze-aws."),
        )));
    }
    let caller = Caller {
        access_key_id: claim.access_key_id,
        account_id: shared.config.account_id.clone(),
        region: claim.region,
        endpoint: endpoint_of(parts),
        service: None,
        session_token: session_token(parts),
    };
    let call = decode_call(entry, op, id, caller, bytes).map_err(|e| fail(&e))?;
    let output = entry.service.call(call).await.map_err(|e| fail(&e))?;
    let encoded = encode_output(entry, op, &output.arena, output.root).map_err(|e| fail(&e))?;
    Ok(json::success(version, id, encoded))
}

/// What a service whose protocol this gateway does not speak yet is told.
fn unserved_protocol(meta: &aws_schema::meta::ServiceMeta) -> ApiError {
    let wire = if meta.speaks(Protocol::AwsQuery) {
        "awsQuery"
    } else {
        "a REST or CBOR protocol"
    };
    ApiError::new(
        501,
        "NotImplemented",
        format!(
            "{} speaks {wire}, which this build does not serve yet.",
            meta.sdk_id
        ),
    )
}

/// The session token the request carried.
pub(crate) fn session_token(parts: &Parts) -> Option<String> {
    parts
        .headers
        .get("x-amz-security-token")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// The scheme and host the caller used, for URLs in answers.
pub(crate) fn endpoint_of(parts: &Parts) -> String {
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
pub(crate) fn authenticate(
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
        s3: signing_name == "s3",
        max_skew_secs: shared.config.max_skew_secs,
    };
    let now = SystemClock.now_ms() / 1000;
    begin(parts, now, endpoint, creds.as_ref()).map(Some)
}

pub(crate) async fn read_body(body: Incoming, limit: usize) -> Result<Bytes, ApiError> {
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
    // An operation whose output is an empty structure answers with no tree at all.
    if root == NIL {
        return Ok(b"{}".to_vec());
    }
    let mut out = Vec::new();
    encode(tables, shape, arena, "", root, &mut out)
        .map_err(|_| ApiError::internal("The response could not be written."))?;
    Ok(out)
}
