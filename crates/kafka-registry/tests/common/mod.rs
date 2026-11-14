//! What the integration tests share: a registry over memory, its API, and a way to send it a request.

#![allow(dead_code, unreachable_pub)]

use std::sync::Arc;

use kafka_registry::api::{Api, Body, CONTENT_TYPE, Options, Request, Response};
use kafka_registry::registry::Registry;
use kafka_registry::store::MemoryStorage;

/// The host names the oracle's tests allow beyond the always-allowed ones.
pub fn test_hosts() -> Vec<String> {
    vec!["example.com".into(), "registry".into()]
}

pub fn registry() -> Arc<Registry> {
    Arc::new(Registry::open(Box::new(MemoryStorage::default())).unwrap())
}

pub fn api() -> Api {
    Api::new(
        registry(),
        Options {
            auth: None,
            allowed_hosts: test_hosts(),
        },
    )
}

/// A request with a body sent as the registry's own type (`ct` empty), another type, or none (`"none"`).
pub fn request(method: &str, target: &str, body: &str, ct: &str) -> Request {
    let mut r = Request::new(method, target);
    r.body = Body::Read(body.as_bytes().to_vec());
    r.content_type = match ct {
        "none" => None,
        "" if body.is_empty() => None,
        "" => Some(CONTENT_TYPE.to_owned()),
        other => Some(other.to_owned()),
    };
    r
}

/// Sends a request and returns the status and the body as text.
pub fn send(api: &Api, method: &str, target: &str, body: &str) -> (u16, String) {
    let resp = api.serve(&request(method, target, body, ""));
    (
        resp.status,
        String::from_utf8_lossy(&resp.body).into_owned(),
    )
}

pub fn serve(api: &Api, req: &Request) -> Response {
    api.serve(req)
}

/// A body with a schema of a type.
pub fn typed_body(kind: &str, schema: &str) -> String {
    serde_json::json!({ "schema": schema, "schemaType": kind }).to_string()
}

/// A body with an Avro schema.
pub fn schema_body(schema: &str) -> String {
    serde_json::json!({ "schema": schema }).to_string()
}

/// The `id` of a registration's answer.
pub fn id_of(body: &str) -> u64 {
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    v["id"].as_u64().unwrap()
}
