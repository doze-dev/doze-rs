//! The HTTP and WebSocket API (v2) control plane. An API is one document: `api` (what `GetApi` shows) beside `routes`,
//! `integrations`, `authorizers`, `stages`, `deployments`, `models`, and the route and integration responses of WebSocket APIs. The
//! request's body members are lowerCamel and its path labels UpperCamel (the model's own spelling), so a record copies the members
//! that begin with a lower-case letter.

mod apis;
mod parts;
mod stages;

pub(crate) use apis::new_api;

use aws_model_apigatewayv2::Operation;
use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::core::Core;
use crate::doc::Doc;
use crate::errors::{no_api, not_found, refused};
use crate::req::{Req, Res};

/// The operations not built, and why: portals, portal products and pages, routing rules (the developer-portal surface: no local
/// counterpart), VPC links (no VPC), and `GetModelTemplate` (a console scaffold).
pub(crate) fn built(op: Operation) -> bool {
    let name = format!("{op:?}");
    !(name.contains("Portal")
        || name.contains("ProductPage")
        || name.contains("ProductRestEndpointPage")
        || name.contains("RoutingRule")
        || name.contains("VpcLink")
        || op == Operation::GetModelTemplate)
}

/// Runs operation `op`.
pub(crate) fn dispatch(core: &mut Core, op: u32, r: &Req) -> Res {
    let Some(op) = Operation::from_index(op) else {
        return Err(refused("This operation"));
    };
    if !built(op) {
        return Err(refused(&format!("{op:?}")));
    }
    if let Some(res) = apis::dispatch(core, op, r) {
        return res;
    }
    if let Some(res) = stages::dispatch(core, op, r) {
        return res;
    }
    parts::dispatch(core, op, r).unwrap_or_else(|| Err(refused(&format!("{op:?}"))))
}

/// The API the request names (`ApiId`).
pub(crate) fn api<'a>(core: &'a Core, r: &Req) -> Result<&'a Value, ApiError> {
    let id = r.s("ApiId");
    core.http
        .get(id)
        .map(|d| &d.0)
        .ok_or_else(|| no_api(r.account(), id))
}

/// The API the request names, to change.
pub(crate) fn api_mut<'a>(core: &'a mut Core, r: &Req) -> Result<&'a mut Value, ApiError> {
    let id = r.s("ApiId");
    match core.http.get_mut(id) {
        Some(Doc(v)) => Ok(v),
        None => Err(no_api(r.account(), id)),
    }
}

/// The request's body members (those spelt lowerCamel), without the ones a record does not keep.
pub(crate) fn members(r: &Req, except: &[&str]) -> Map<String, Value> {
    r.body
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(k, v)| {
            k.starts_with(|c: char| c.is_ascii_lowercase())
                && !v.is_null()
                && !except.contains(&k.as_str())
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// A missing part of an API `(memory)` for the words.
pub(crate) fn missing(thing: &str, id: &str) -> ApiError {
    not_found(format!("Invalid {thing} identifier specified {id}"))
}

/// One page of a list, as `{items, nextToken}` (`MaxResults` and `NextToken`, the offset as text).
pub(crate) fn page(items: Vec<Value>, r: &Req) -> Value {
    let max = r
        .opt("MaxResults")
        .and_then(|m| m.parse::<usize>().ok())
        .filter(|m| *m > 0)
        .unwrap_or(usize::MAX);
    let from = r
        .opt("NextToken")
        .and_then(|p| p.parse::<usize>().ok())
        .unwrap_or(0);
    let total = items.len();
    let shown: Vec<Value> = items.into_iter().skip(from).take(max).collect();
    let next = from + shown.len();
    let mut out = json!({"items": shown});
    if next < total {
        crate::doc::put(&mut out, "nextToken", json!(next.to_string()));
    }
    out
}
