//! A REST request read into the operation's input: the labels, the query, the headers and the body, in the shape the model says.

use aws_auth::claim::Claim;
use aws_schema::arena::{Arena, NIL};
use aws_schema::json::decode;
use aws_schema::tables::Tables;
use aws_wire::rest::fold::fold_enum_headers;
use aws_wire::rest::jsonin;
use aws_wire::rest::request::{Parts as Input, ReadError, to_json};
use aws_wire::rest::route::Matched;
use aws_wire::service::{Call, Caller};
use aws_wire::{ApiError, RequestId};
use http::request::Parts;

use crate::pipeline::{Entry, session_token};

/// What of the request the call is made from.
pub(super) struct Head<'a> {
    pub(super) id: &'a RequestId,
    pub(super) parts: &'a Parts,
    pub(super) host: &'a str,
    pub(super) account: &'a str,
    pub(super) matched: &'a Matched,
    pub(super) query: &'a [(String, String)],
    pub(super) body: &'a [u8],
    pub(super) json: bool,
}

/// The input of a restJson1 operation; the body is only named for the error (a body that is not JSON is the service's to word).
fn json_in(tables: &Tables, shape: u32, p: &Input<'_>) -> Option<serde_json::Value> {
    jsonin::to_json(tables, shape, p).ok()
}

pub(super) fn decode_call(entry: &Entry, h: &Head<'_>, claim: Claim) -> Result<Call, ApiError> {
    let (matched, parts, id) = (h.matched, h.parts, h.id);
    let tables = entry.service.tables();
    let input = tables.op(matched.op).and_then(|o| o.input);
    let mut arena = Arena::new();
    let (text, root) = match input {
        Some(shape) => {
            let p = Input {
                labels: &matched.labels,
                query: h.query,
                headers: &parts.headers,
                body: h.body,
            };
            let value = if h.json {
                json_in(tables, shape, &p).ok_or_else(|| {
                    entry
                        .service
                        .invalid(&aws_schema::json::Violation::Syntax { at: 0 })
                })?
            } else {
                to_json(tables, shape, &p).map_err(|ReadError::MalformedXml| {
                    ApiError::new(
                        400,
                        "MalformedXML",
                        "The XML you provided was not well-formed or did not validate against our published schema",
                    )
                })?
            };
            let mut value = value;
            let blind = entry.service.case_blind_headers();
            if !blind.is_empty() {
                fold_enum_headers(tables, shape, blind, &mut value);
            }
            let text = value.to_string();
            let root = decode(tables, &entry.patterns, shape, &text, &mut arena)
                .map_err(|v| entry.service.invalid(&v))?;
            (text, root)
        }
        None => (String::new(), NIL),
    };
    let call = Call {
        op: matched.op,
        id: id.clone(),
        caller: Caller {
            access_key_id: claim.access_key_id,
            account_id: h.account.to_owned(),
            region: claim.region,
            endpoint: format!("http://{}", h.host),
            service: None,
            session_token: session_token(parts),
        },
        body: text,
        arena,
        root,
    };
    crate::observe::input(parts, entry, &call);
    Ok(call)
}
