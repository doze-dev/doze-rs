//! A request in the awsQuery protocol: the same steps as a JSON one (authenticate, read, route, decode and check against the model,
//! call, answer), with the operation named by the form's `Action` and the answer in XML.

use aws_auth::claim::Claim;
use aws_schema::arena::{Arena, NIL};
use aws_schema::enc::encode;
use aws_schema::json::decode;
use aws_wire::service::{Call, Caller};
use aws_wire::{ApiError, Family, RequestId, from_auth, query};
use http::request::Parts;
use hyper::body::Incoming;
use serde_json::Value;

use crate::fault::Step;
use crate::pipeline::{Entry, Fail, Out, Shared, authenticate, early, endpoint_of, read_body};

/// Answers a Query request to `entry`.
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
        crate::observe::failed(parts, e);
        Box::new(query::failure(meta, tables, id, e))
    };
    let authed = authenticate(shared, parts, claim.service.as_str())
        .await
        .map_err(|e| fail(&from_auth(&e, Family::Query)))?;
    let bytes = read_body(body, shared.max_body_bytes())
        .await
        .map_err(|e| fail(&e))?;
    if let Some(pending) = authed {
        let hash = pending
            .needs_body_hash()
            .then(|| aws_auth::key::sha256_hex(&bytes));
        pending
            .finish(parts, hash.as_deref())
            .map_err(|e| fail(&from_auth(&e, Family::Query)))?;
    }
    let form = form_of(parts, &bytes).map_err(|e| fail(&e))?;
    answer(shared, entry, id, parts, claim, &form).await
}

/// The request's form: its query string and its body.
fn form_of(
    parts: &Parts,
    bytes: &[u8],
) -> Result<std::collections::BTreeMap<String, String>, ApiError> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        ApiError::new(
            400,
            "MalformedInput",
            "The request body is not valid UTF-8.",
        )
    })?;
    let mut form = query::parse_form(parts.uri.query().unwrap_or_default());
    form.extend(query::parse_form(text));
    Ok(form)
}

/// A request with no signature: answered when it is a Query `Action` whose model marks it as taking none ([`Service::unsigned`]),
/// for the service that has it; anything else is refused as AWS refuses a request with no authentication.
pub(crate) async fn unsigned(
    shared: &Shared,
    id: &RequestId,
    parts: &Parts,
    body: Incoming,
) -> Result<Out, Fail> {
    let missing = || {
        let e = from_auth(&aws_auth::AuthError::Missing, Family::Coral);
        early(id, parts, &e)
    };
    let form_post = parts
        .headers
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/x-www-form-urlencoded"));
    if !form_post {
        return Err(missing());
    }
    let bytes = read_body(body, shared.max_body_bytes())
        .await
        .map_err(|e| early(id, parts, &e))?;
    let form = form_of(parts, &bytes).map_err(|e| early(id, parts, &e))?;
    let action = query::action(&form).unwrap_or_default();
    let entry = shared
        .entries()
        .iter()
        .find(|e| {
            e.service
                .meta()
                .speaks(aws_schema::meta::Protocol::AwsQuery)
                && e.service
                    .tables()
                    .find_op(action)
                    .is_some_and(|op| e.service.unsigned(op))
        })
        .ok_or_else(missing)?;
    let claim = crate::presign::unsigned_claim(
        entry.service.meta().signing_name,
        &shared.config.default_region,
    );
    crate::observe::service(parts, entry, &claim);
    answer(shared, entry, id, parts, claim, &form).await
}

/// Answers a Query request whose signature (if it needed one) has been checked, from its form.
async fn answer(
    shared: &Shared,
    entry: &Entry,
    id: &RequestId,
    parts: &Parts,
    claim: Claim,
    form: &std::collections::BTreeMap<String, String>,
) -> Result<Out, Fail> {
    let (meta, tables) = (entry.service.meta(), entry.service.tables());
    let fail = |e: &ApiError| -> Fail {
        crate::observe::failed(parts, e);
        Box::new(query::failure(meta, tables, id, e))
    };
    let name = query::action(form).ok_or_else(|| {
        fail(&ApiError::new(
            400,
            "MissingAction",
            "Request is missing Action or Operation parameter",
        ))
    })?;
    // An action outside the model is AWS's `InvalidAction`, status 404 (recorded on IAM 2026-11-15; the sentence is memory), whatever
    // its name: doze's own helpers live under `/_doze/`, never on an AWS endpoint.
    let op = tables.find_op(name).ok_or_else(|| {
        fail(&ApiError::new(
            404,
            "InvalidAction",
            format!(
                "Could not find operation {name} for version {}",
                meta.version
            ),
        ))
    })?;
    crate::observe::operation(parts, entry, op);
    if !entry.service.serves(op) {
        return Err(fail(&ApiError::new(
            501,
            "NotImplemented",
            format!("{name} is not built in doze-aws."),
        )));
    }
    let call =
        decode_call(entry, op, id, caller(shared, parts, claim), form).map_err(|e| fail(&e))?;
    crate::observe::input(parts, entry, &call);
    match crate::fault::apply(shared, parts).await {
        Step::Go => {}
        Step::Fail(e) => return Err(fail(&e)),
        Step::Drop => return Err(Box::new(crate::fault::marked_dropped())),
    }
    let output = entry.service.call(call).await.map_err(|e| fail(&e))?;
    crate::observe::output(parts, entry, &output);
    let json = output_json(entry, op, &output.arena, output.root).map_err(|e| fail(&e))?;
    Ok(query::success(meta, tables, op, &json, id))
}

fn caller(shared: &Shared, parts: &Parts, claim: Claim) -> Caller {
    Caller {
        access_key_id: claim.access_key_id,
        account_id: shared.account_id().to_owned(),
        region: claim.region,
        endpoint: endpoint_of(parts),
        service: None,
        session_token: crate::pipeline::session_token(parts),
    }
}

/// The form read into the operation's input shape, checked against the model.
fn decode_call(
    entry: &Entry,
    op: u32,
    id: &RequestId,
    caller: Caller,
    form: &std::collections::BTreeMap<String, String>,
) -> Result<Call, ApiError> {
    let tables = entry.service.tables();
    let input = tables.op(op).and_then(|o| o.input);
    let mut arena = Arena::new();
    let (body, root) = match input {
        Some(shape) => {
            let body = query::to_json(tables, shape, form).to_string();
            let root = decode(tables, &entry.patterns, shape, &body, &mut arena)
                .map_err(|v| entry.service.invalid(&v))?;
            (body, root)
        }
        None => (String::new(), NIL),
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

/// The service's answer as JSON, for the XML writer.
fn output_json(entry: &Entry, op: u32, arena: &Arena, root: u32) -> Result<Value, ApiError> {
    let tables = entry.service.tables();
    let Some(shape) = tables.op(op).and_then(|o| o.output).filter(|_| root != NIL) else {
        return Ok(Value::Object(serde_json::Map::new()));
    };
    let mut bytes = Vec::new();
    encode(tables, shape, arena, "", root, &mut bytes)
        .map_err(|_| ApiError::internal("The response could not be written."))?;
    serde_json::from_slice(&bytes)
        .map_err(|_| ApiError::internal("The response could not be read back."))
}
