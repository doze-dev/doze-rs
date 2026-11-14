//! One request, from the socket to the answer.

use std::collections::HashMap;

use crate::body::RespBody;
use aws_auth::AuthError;
use aws_auth::claim::Claim;
use aws_schema::arena::{Arena, NIL};
use aws_schema::enc::encode;
use aws_schema::json::decode;
use aws_schema::meta::Protocol;
use aws_schema::patterns::Patterns;
use aws_wire::service::{Call, Caller, Service};
use aws_wire::{ApiError, Family, RequestId, from_auth, json};
use bytes::Bytes;
use http::Response;
use http::request::Parts;
use hyper::body::Incoming;

pub(crate) use crate::head::{
    authenticate, caller_of, endpoint_of, inflate, read_body, session_token,
};
use crate::target::{extension, target_prefix};
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
    /// Every service by the name of its service shape (the `X-Amz-Target` prefix of the JSON protocols): two services that share a
    /// signing name (DynamoDB and DynamoDB Streams both sign as `dynamodb`) are told apart by it.
    by_target: HashMap<&'static str, usize>,
    pub(crate) config: Config,
    /// Where sessions on upgraded connections run, so a shutdown ends them too.
    tasks: doze_runtime::Tasks,
    /// When the gateway started (for `/_doze/health`).
    started: std::time::Instant,
}

impl Shared {
    pub(crate) fn new(entries: Vec<Entry>, config: Config, tasks: doze_runtime::Tasks) -> Self {
        let by_signing_name = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.service.meta().signing_name, i))
            .collect();
        let by_target = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.service.meta().shape_name, i))
            .collect();
        Self {
            entries,
            by_signing_name,
            by_target,
            config,
            tasks,
            started: std::time::Instant::now(),
        }
    }

    /// When the gateway started.
    pub(crate) fn started(&self) -> std::time::Instant {
        self.started
    }

    /// The gateway's task set.
    pub(crate) fn tasks(&self) -> &doze_runtime::Tasks {
        &self.tasks
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

    /// The services that own addresses of their own.
    pub(crate) fn webs(&self) -> impl Iterator<Item = &dyn aws_wire::service::Web> {
        self.entries.iter().filter_map(|e| e.service.web())
    }

    pub(crate) fn entry(&self, signing_name: &str) -> Option<&Entry> {
        self.by_signing_name
            .get(signing_name)
            .and_then(|&i| self.entries.get(i))
    }

    /// The service a JSON request is for: the one whose service shape its `X-Amz-Target` names, when that service signs as
    /// `signing_name`; otherwise the one registered under the signing name.
    pub(crate) fn entry_for(&self, signing_name: &str, parts: &Parts) -> Option<&Entry> {
        let by_target = target_prefix(parts)
            .and_then(|prefix| self.by_target.get(prefix))
            .and_then(|&i| self.entries.get(i))
            .filter(|e| e.service.meta().signing_name == signing_name);
        by_target.or_else(|| self.entry(signing_name))
    }

    /// The REST service a request signed as `signing_name` is for. Two models can share a signing name (API Gateway's REST and
    /// HTTP APIs both sign as `apigateway`): the one whose routes take the method and path is the one; otherwise the last registered.
    pub(crate) fn rest_entry(&self, signing_name: &str, parts: &Parts) -> Option<&Entry> {
        let mut same = self
            .entries
            .iter()
            .filter(|e| e.service.meta().signing_name == signing_name);
        let first = same.next()?;
        let Some(second) = same.next() else {
            return Some(first);
        };
        let query = crate::presign::pairs(parts.uri.query().unwrap_or_default());
        [first, second]
            .into_iter()
            .chain(same)
            .find(|e| {
                e.router(e.service.tables())
                    .find(parts.method.as_str(), parts.uri.path(), &query, |h| {
                        parts.headers.contains_key(h)
                    })
                    .is_some()
            })
            .or_else(|| self.entry(signing_name))
    }
}

pub(crate) type Out = Response<Vec<u8>>;
/// An error answer, boxed so the `Err` of a `Result` stays small.
pub(crate) type Fail = Box<Out>;

/// Answers a request. Never fails: a failure is an error answer.
pub(crate) async fn respond(shared: &Shared, req: http::Request<Incoming>) -> Response<RespBody> {
    let id = RequestId::new();
    let (mut parts, body) = req.into_parts();
    if let Some(health) = crate::health::answer(shared, &parts) {
        return health;
    }
    let body = match crate::web::serve(shared, &id, &mut parts, body).await {
        Ok(response) => return response,
        Err(body) => body,
    };
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
    // A request with no signature at all, to a gateway that trusts, is a person with curl or a browser: S3 serves it as the local
    // account.
    let claim = claim.or_else(|| {
        shared
            .trusts()
            .then(|| crate::presign::anonymous_claim(&parts))
            .flatten()
    });
    // Neither an unsigned request nor a SigV2 URL names a region: they are for the configured default.
    let claim = claim.map(|mut c| {
        if c.scope_date.is_empty() {
            c.region.clone_from(&shared.config.default_region);
        }
        c
    });
    if let Some(claim) = claim
        && let Some(entry) = shared.rest_entry(&claim.service, &parts).filter(|e| {
            let meta = e.service.meta();
            meta.speaks(Protocol::RestXml) || meta.speaks(Protocol::RestJson1)
        })
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
    let entry = shared.entry_for(&claim.service, parts).ok_or_else(|| {
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
    if aws_wire::cbor::is_request(parts) && meta.speaks(Protocol::Rpcv2Cbor) {
        return crate::cbor::handle(shared, entry, id, parts, body, claim).await;
    }
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
        .await
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
    // An operation the service adds to its model (SQS's `DozePeek`) borrows a model operation's shapes.
    let (op, shapes) = match json::route(meta, tables, parts) {
        Ok(op) => (op, op),
        Err(e) => extension(entry, parts).ok_or_else(|| fail(&e))?,
    };
    if op == shapes
        && let Some(refusal) = unbuilt(entry, op)
    {
        return Err(fail(&refusal));
    }
    let bytes = inflate(parts, bytes, shared.max_body_bytes()).map_err(|e| fail(&e))?;
    let caller = caller_of(shared, parts, claim);
    let mut call = decode_call(entry, shapes, id, caller, bytes).map_err(|e| fail(&e))?;
    call.op = op;
    let output = entry.service.call(call).await.map_err(|e| fail(&e))?;
    let encoded = encode_output(entry, shapes, &output.arena, output.root).map_err(|e| fail(&e))?;
    Ok(json::success(version, id, encoded))
}

/// The refusal for an operation the service does not build (501, by name), before its body is read.
pub(crate) fn unbuilt(entry: &Entry, op: u32) -> Option<ApiError> {
    if entry.service.serves(op) {
        return None;
    }
    let name = entry.service.tables().op(op).map_or("", |o| o.name);
    Some(ApiError::new(
        501,
        "NotImplemented",
        format!("{name} is not built in doze-aws."),
    ))
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

/// An error raised before a service is known.
fn early(id: &RequestId, err: &ApiError) -> Fail {
    Box::new(json::generic_failure(json::Version::V10, id, err))
}

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
            decode(tables, &entry.patterns, shape, text, &mut arena)
                .map_err(|v| entry.service.invalid_in(&v, text))?
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
