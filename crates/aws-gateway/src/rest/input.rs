//! A REST request read into the operation's input: the labels, the query, the headers and the body, in the shape the model says.

use aws_auth::claim::Claim;
use aws_schema::arena::{Arena, NIL};
use aws_schema::json::{Violation, decode};
use aws_schema::tables::{K_BLOB, Tables};
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

/// The input member bound to the whole body when it is a blob that is not a stream (Lambda's `Invoke` `Payload`).
fn blob_payload(tables: &Tables, shape: u32) -> Option<u32> {
    let sh = tables.shape(shape)?;
    (sh.first..sh.first + sh.count).find(|&i| {
        tables
            .member(i)
            .is_some_and(|m| m.bind == 6 && m.kind == K_BLOB)
    })
}

/// The body as the blob member `member`, held to the member's length bounds as the decoder holds a blob. Before, the body went to
/// base64 in a JSON value, to its text, and back through the decoder: three passes and three copies of a 6 MB `Invoke` payload where
/// this makes one copy.
fn add_payload(
    tables: &Tables,
    member: u32,
    body: &[u8],
    arena: &mut Arena,
    root: u32,
) -> Result<(), Violation> {
    if body.is_empty() {
        return Ok(());
    }
    let n = body.len() as u64;
    let cons = tables.member(member).and_then(|m| tables.cons(m.cons));
    if cons.is_some_and(|c| c.len_min.is_some_and(|a| n < a) || c.len_max.is_some_and(|b| n > b)) {
        return Err(Violation::Length {
            member,
            value: None,
        });
    }
    if arena.add_blob(root, member, body) {
        Ok(())
    } else {
        Err(Violation::Table)
    }
}

pub(super) fn decode_call(entry: &Entry, h: &Head<'_>, claim: Claim) -> Result<Call, ApiError> {
    let (matched, parts, id) = (h.matched, h.parts, h.id);
    let tables = entry.service.tables();
    let input = tables.op(matched.op).and_then(|o| o.input);
    let mut arena = Arena::new();
    let (text, root) = match input {
        Some(shape) => {
            // A blob that is the whole body goes into the tree as its bytes after the rest is decoded (`add_payload`).
            let aside = (h.json && entry.service.payload_aside(matched.op))
                .then(|| blob_payload(tables, shape))
                .flatten();
            let p = Input {
                labels: &matched.labels,
                query: h.query,
                headers: &parts.headers,
                body: if aside.is_some() { &[] } else { h.body },
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
            if let Some(member) = aside {
                add_payload(tables, member, h.body, &mut arena, root)
                    .map_err(|v| entry.service.invalid(&v))?;
            }
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use aws_model_lambda::tables::TABLES;
    use aws_schema::arena::Val;
    use aws_schema::patterns::Patterns;

    use super::*;

    #[test]
    fn a_blob_body_joins_the_tree_as_its_bytes() {
        let shape = TABLES
            .op(TABLES.find_op("Invoke").unwrap())
            .unwrap()
            .input
            .unwrap();
        let member = blob_payload(&TABLES, shape).unwrap();
        assert_eq!(TABLES.member(member).unwrap().name, "Payload");
        let text = r#"{"FunctionName":"f"}"#;
        let mut arena = Arena::new();
        let root = decode(&TABLES, &Patterns::new(&TABLES), shape, text, &mut arena).unwrap();
        add_payload(&TABLES, member, br#"{"n":1}"#, &mut arena, root).unwrap();
        let Some(Val::Obj(first)) = arena.node(root).map(|n| n.val) else {
            panic!("the input is a structure");
        };
        let added = arena.node(first).unwrap();
        assert_eq!(added.member, member);
        assert_eq!(arena.bytes_of(added.val), Some(&br#"{"n":1}"#[..]));
        // The members decoded from the text are still there after it.
        let name = arena.node(added.next).unwrap();
        assert_eq!(arena.str_of(text, name.val), Some("f"));
        // An empty body adds nothing.
        let before = arena.len();
        add_payload(&TABLES, member, b"", &mut arena, root).unwrap();
        assert_eq!(arena.len(), before);
    }
}
