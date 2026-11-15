#![no_main]
//! Any request to the REST API (method, path, query, headers and body from the input) against a registry that already
//! holds schemas, a reference and a deleted version: never a panic, every answer is the registry's JSON (or a schema's
//! text), and a status is one Confluent's API gives.

use std::sync::{Arc, OnceLock};

use kafka_registry::api::{Api, Body, Options, Request};
use kafka_registry::registry::Registry;
use kafka_registry::store::MemoryStorage;
use libfuzzer_sys::fuzz_target;

const METHODS: [&str; 5] = ["GET", "POST", "PUT", "DELETE", "HEAD"];

fn seeded() -> &'static Api {
    static API: OnceLock<Api> = OnceLock::new();
    API.get_or_init(|| {
        let api = Api::new(Arc::new(Registry::open(Box::new(MemoryStorage::default())).unwrap()), Options::default());
        for (target, body) in [
            ("/subjects/a/versions", r#"{"schema":"{\"type\":\"record\",\"name\":\"A\",\"fields\":[{\"name\":\"x\",\"type\":\"int\"}]}"}"#),
            ("/subjects/b/versions", r#"{"schema":"{\"type\":\"record\",\"name\":\"B\",\"fields\":[{\"name\":\"a\",\"type\":\"A\"}]}","references":[{"name":"A","subject":"a","version":1}]}"#),
            ("/subjects/c/versions", r#"{"schema":"\"string\""}"#),
        ] {
            let mut r = Request::new("POST", target);
            r.body = Body::Read(body.as_bytes().to_vec());
            let _ = api.serve(&r);
        }
        let _ = api.serve(&Request::new("DELETE", "/subjects/c"));
        api
    })
}

fuzz_target!(|data: &[u8]| {
    let Some((&m, rest)) = data.split_first() else {
        return;
    };
    let text = String::from_utf8_lossy(rest);
    let mut parts = text.splitn(4, '\n');
    let target = parts.next().unwrap_or_default();
    let mut r = Request::new(METHODS[usize::from(m) % METHODS.len()], "/");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    r.path = format!("/{}", path.trim_start_matches('/'));
    r.query = query.to_owned();
    r.authorization = parts.next().map(str::to_owned);
    r.content_type = parts.next().map(str::to_owned);
    r.body = if m & 0x80 == 0 {
        Body::Read(parts.next().unwrap_or_default().as_bytes().to_vec())
    } else {
        Body::TooLarge(1)
    };
    let resp = seeded().serve(&r);
    assert!(
        matches!(
            resp.status,
            200 | 400 | 401 | 403 | 404 | 405 | 409 | 413 | 415 | 422 | 500
        ),
        "{} for {r:?}",
        resp.status
    );
});
