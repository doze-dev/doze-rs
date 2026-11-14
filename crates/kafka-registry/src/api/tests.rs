//! The API's own rules, ported from the Go edition's `hostguard_test.go`, `api_log_test.go`, `bounds_test.go`,
//! `limits_exact_test.go` and `doze_ext_test.go` (the oracle's answers kept). The Go edition also answers to every name
//! under `.doze`; this edition answers to the names the operator lists (REG-D8), so those cases are not here.

use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::registry::Registry;
use crate::store::MemoryStorage;

const USER1: &str = r#"{"type":"record","name":"User","fields":[{"name":"id","type":"long"}]}"#;
const USER_BAD: &str = r#"{"type":"record","name":"User","fields":[{"name":"id","type":"long"},{"name":"email","type":"string"}]}"#;

fn api_with(options: Options) -> Api {
    Api::new(
        Arc::new(Registry::open(Box::new(MemoryStorage::default())).unwrap()),
        options,
    )
}

fn hosts() -> Vec<String> {
    vec!["example.com".into(), "registry".into()]
}

fn plain() -> Api {
    api_with(Options {
        auth: None,
        allowed_hosts: hosts(),
    })
}

fn guarded(password: &'static str) -> Api {
    api_with(Options {
        auth: Some(Arc::new(move |_: &str, p: &str| p == password)),
        allowed_hosts: hosts(),
    })
}

fn schema_body(s: &str) -> String {
    serde_json::json!({ "schema": s }).to_string()
}

fn req(method: &str, target: &str, body: &str) -> Request {
    let mut r = Request::new(method, target);
    if !body.is_empty() {
        r.content_type = Some(CONTENT_TYPE.into());
    }
    r.body = Body::Read(body.as_bytes().to_vec());
    r
}

fn at_host(api: &Api, method: &str, target: &str, host: &str, body: &str) -> Response {
    let mut r = req(method, target, body);
    r.host = host.into();
    api.serve(&r)
}

fn text(resp: &Response) -> String {
    String::from_utf8_lossy(&resp.body).into_owned()
}

fn as_user(api: &Api, target: &str, user: &str, pass: &str) -> Response {
    let mut r = req("GET", target, "");
    // "alice:<pass>" in base64, by the decoder's own table run backwards: built by hand for the three passwords used.
    r.authorization = Some(match (user, pass) {
        ("alice", "wrong") => "Basic YWxpY2U6d3Jvbmc=".into(),
        ("alice", "secret") => "Basic YWxpY2U6c2VjcmV0".into(),
        _ => "Basic YWxpY2U6cHc=".into(),
    });
    api.serve(&r)
}

#[test]
fn a_request_for_a_foreign_host_is_refused_and_changes_nothing() {
    let a = api_with(Options::default());
    assert_eq!(
        at_host(
            &a,
            "POST",
            "/subjects/orders/versions",
            "127.0.0.1:8081",
            &schema_body(USER1)
        )
        .status,
        200
    );
    let foreign = "attacker.example:8081";
    for (method, path, body) in [
        ("GET", "/", ""),
        ("GET", "/subjects", ""),
        ("GET", "/schemas/ids/1", ""),
        ("GET", "/no/such/route", ""),
        (
            "POST",
            "/subjects/pwned/versions",
            schema_body(USER1).as_str(),
        ),
        ("PUT", "/config", r#"{"compatibility":"NONE"}"#),
        ("PUT", "/mode", r#"{"mode":"READONLY"}"#),
        ("DELETE", "/subjects/orders", ""),
        ("OPTIONS", "/subjects", ""),
    ] {
        let resp = at_host(&a, method, path, foreign, body);
        let v: serde_json::Value = serde_json::from_slice(&resp.body).unwrap();
        assert_eq!(
            (resp.status, v["error_code"].as_u64()),
            (403, Some(403)),
            "{method} {path}"
        );
        let msg = v["message"].as_str().unwrap();
        assert!(
            msg.contains("\"attacker.example:8081\"")
                && msg.contains("--console-allow-host=attacker.example"),
            "{msg}"
        );
        assert!(!text(&resp).contains("User") && !text(&resp).contains("orders"));
    }
    let local = |path| text(&at_host(&a, "GET", path, "127.0.0.1:8081", ""));
    assert_eq!(local("/subjects"), "[\"orders\"]\n");
    assert!(local("/config").contains("BACKWARD") && local("/mode").contains("READWRITE"));
}

#[test]
fn the_registrys_own_hosts_are_served_and_lookalikes_are_not() {
    let a = api_with(Options {
        auth: None,
        allowed_hosts: vec!["host.docker.internal".into()],
    });
    for host in [
        "127.0.0.1:8081",
        "127.0.0.1",
        "localhost:8081",
        "localhost",
        "[::1]:8081",
        "[::1]",
        "192.168.1.20:8081",
        "0.0.0.0:8081",
        "LOCALHOST:8081",
        "localhost.:8081",
        "host.docker.internal:8081",
        "HOST.docker.internal.:8081",
    ] {
        assert_eq!(
            at_host(&a, "GET", "/subjects", host, "").status,
            200,
            "{host}"
        );
    }
    for host in [
        "evil.example:8081@localhost",
        "localhost:8081@evil.example",
        "localhost@evil.example",
        "127.0.0.1.evil.example:8081",
        "host.docker.internal.evil.example",
        "evil.host.docker.internal",
        "localhost:8081 evil.example",
        "",
        "schema.shop.doze",
    ] {
        assert_eq!(
            at_host(&a, "GET", "/subjects", host, "").status,
            403,
            "{host}"
        );
    }
    let open = api_with(Options {
        auth: None,
        allowed_hosts: vec!["*".into()],
    });
    assert_eq!(
        at_host(&open, "GET", "/subjects", "anything.example:8081", "").status,
        200
    );
}

#[test]
fn the_host_is_judged_before_the_credentials() {
    let a = guarded("pw");
    let mut refusals = Vec::new();
    for auth in [
        None,
        Some("Basic YWxpY2U6bm9wZQ=="),
        Some("Basic YWxpY2U6cHc="),
    ] {
        let mut r = req("POST", "/subjects/orders/versions", &schema_body(USER1));
        r.host = "attacker.example:8081".into();
        r.authorization = auth.map(str::to_owned);
        let resp = a.serve(&r);
        assert_eq!((resp.status, resp.challenge), (403, false));
        refusals.push(resp.body);
    }
    assert!(
        refusals.windows(2).all(|w| w[0] == w[1]),
        "the refusal says whether the credentials were right"
    );
    let resp = a.serve(&req(
        "POST",
        "/subjects/orders/versions",
        &schema_body(USER1),
    ));
    assert_eq!((resp.status, resp.challenge), (401, true));
    assert_eq!(text(&as_user(&a, "/subjects", "alice", "pw")), "[]\n");
}

#[test]
fn repeated_sign_in_failures_are_slowed_and_a_success_clears_them() {
    let a = guarded("secret");
    let waits: Vec<Duration> = (0..7)
        .map(|_| as_user(&a, "/subjects", "alice", "wrong").delay)
        .collect();
    assert_eq!(&waits[..5], &[Duration::ZERO; 5]);
    assert_eq!(
        &waits[5..],
        &[Duration::from_millis(100), Duration::from_millis(200)]
    );
    let last = (0..40)
        .map(|_| as_user(&a, "/subjects", "alice", "wrong").delay)
        .last()
        .unwrap();
    assert_eq!(last, Duration::from_secs(2));
    assert_eq!(as_user(&a, "/subjects", "alice", "secret").status, 200);
    assert_eq!(
        as_user(&a, "/subjects", "alice", "wrong").delay,
        Duration::ZERO
    );
}

#[test]
fn the_extension_routes_ask_for_the_same_credentials() {
    let a = guarded("pw");
    for (method, path) in [
        ("GET", "/_doze/overview"),
        ("GET", "/_doze/requests"),
        ("GET", "/_doze/subjects/x/versions/1"),
        ("GET", "/_doze/subjects/x/versions/1/resolved"),
        ("POST", "/_doze/subjects/x/check"),
    ] {
        assert_eq!(
            a.serve(&req(method, path, &schema_body(USER1))).status,
            401,
            "{method} {path}"
        );
    }
}

#[test]
fn bodies_are_bounded_by_what_the_route_reads() {
    let a = plain();
    for path in ["/config", "/config/x", "/mode", "/mode/x"] {
        let mut r = req("PUT", path, "");
        r.content_type = Some(CONTENT_TYPE.into());
        r.body = Body::TooLarge(body_limit(path));
        assert_eq!(body_limit(path), MAX_SETTING_BODY);
        assert_eq!(a.serve(&r).status, 413, "{path}");
    }
    let mut r = req("POST", "/subjects/x/versions", "");
    r.content_type = Some(CONTENT_TYPE.into());
    r.body = Body::TooLarge(MAX_SCHEMA_BODY);
    let resp = a.serve(&r);
    assert_eq!(resp.status, 413);
    assert!(text(&resp).contains("20971520"), "{}", text(&resp));
}

#[test]
fn a_page_is_cut_only_by_a_positive_limit() {
    let a = plain();
    for (i, s) in [r#""string""#, r#""int""#, r#""long""#].iter().enumerate() {
        assert_eq!(
            a.serve(&req(
                "POST",
                &format!("/subjects/s{i}/versions"),
                &schema_body(s)
            ))
            .status,
            200
        );
    }
    for (q, want) in [
        ("limit=0", 3),
        ("limit=-1", 3),
        ("limit=2", 2),
        ("limit=3", 3),
        ("offset=0&limit=1", 1),
        ("offset=2", 1),
    ] {
        let body = text(&a.serve(&req("GET", &format!("/schemas?{q}"), "")));
        assert_eq!(body.matches("\"subject\"").count(), want, "{q}: {body}");
    }
}

#[test]
fn the_request_log_keeps_clients_requests_not_the_consoles_and_pages_by_subject() {
    let a = plain();
    let _ = a.serve(&req(
        "POST",
        "/subjects/orders/versions",
        &schema_body(USER1),
    ));
    let _ = a.serve(&req("GET", "/schemas/ids/1?fetchMaxId=true", ""));
    let mut own = req("GET", "/subjects", "");
    own.own = true;
    let _ = a.serve(&own);
    let _ = a.serve(&req("GET", "/_doze/overview", ""));
    assert_eq!(a.logged(), 2);
    let page: serde_json::Value = serde_json::from_slice(
        &a.serve(&req("GET", "/_doze/requests?subject=orders", ""))
            .body,
    )
    .unwrap();
    let paths: Vec<&str> = page["requests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        [
            "/schemas/ids/1?fetchMaxId=true",
            "/subjects/orders/versions"
        ]
    );
    assert_eq!(
        (page["kept"].as_u64(), page["max"].as_u64()),
        (Some(2), Some(2000))
    );
}

#[test]
fn a_check_at_another_level_leaves_the_subjects_and_takes_its_reasons_apart() {
    let a = plain();
    let _ = a.serve(&req("POST", "/subjects/x/versions", &schema_body(USER1)));
    let resp = a.serve(&req(
        "POST",
        "/_doze/subjects/x/check?level=FULL",
        &schema_body(USER_BAD),
    ));
    let got: serde_json::Value = serde_json::from_slice(&resp.body).unwrap();
    assert_eq!(
        (got["compatible"].as_bool(), got["level"].as_str()),
        (Some(false), Some("FULL"))
    );
    assert_eq!(got["compared"], serde_json::json!([1]));
    let reason = &got["findings"][0]["reasons"][0];
    assert_eq!(
        (
            reason["type"].as_str(),
            reason["path"].as_str(),
            reason["info"].as_str()
        ),
        (
            Some("READER_FIELD_MISSING_DEFAULT_VALUE"),
            Some("/fields/1"),
            Some("email")
        )
    );
    let forward = a.serve(&req(
        "POST",
        "/_doze/subjects/x/check?level=forward",
        &schema_body(USER_BAD),
    ));
    assert!(text(&forward).contains("\"compatible\":true"));
    assert!(text(&a.serve(&req("GET", "/config/x?defaultToGlobal=true", ""))).contains("BACKWARD"));
    let bad = a.serve(&req(
        "POST",
        "/_doze/subjects/x/check?level=SIDEWAYS",
        &schema_body(USER_BAD),
    ));
    assert!(text(&bad).contains("42203"));
}

#[test]
fn version_metadata_is_answered_over_http() {
    let a = plain();
    let mut r = req("POST", "/subjects/s/versions", &schema_body(USER1));
    r.user_agent = "x".repeat(300);
    r.peer = "10.1.2.3".into();
    let _ = a.serve(&r);
    let v: serde_json::Value = serde_json::from_slice(
        &a.serve(&req("GET", "/_doze/subjects/s/versions/1", ""))
            .body,
    )
    .unwrap();
    assert_eq!(
        (v["from"].as_str(), v["deleted"].as_bool(), v["id"].as_u64()),
        (Some("10.1.2.3"), Some(false), Some(1))
    );
    assert_eq!(
        v["agent"].as_str().map(str::len),
        Some(256),
        "a long agent is cut"
    );
}

#[test]
fn bodies_of_other_types_are_refused_and_the_mux_answers_in_json() {
    let a = plain();
    for (ct, want) in [
        ("text/plain", 415),
        ("application/x-www-form-urlencoded", 415),
        ("multipart/form-data; boundary=x", 415),
        ("application/json; charset=utf-8", 200),
        ("application/octet-stream", 200),
    ] {
        let mut r = req("POST", "/subjects/ct/versions", &schema_body(USER1));
        r.content_type = Some(ct.into());
        assert_eq!(a.serve(&r).status, want, "{ct}");
    }
    let nf = a.serve(&req("GET", "/contexts", ""));
    assert_eq!(
        (nf.status, text(&nf)),
        (
            404,
            "{\"error_code\":404,\"message\":\"HTTP 404 Not Found\"}\n".into()
        )
    );
    assert_eq!(a.serve(&req("DELETE", "/mode", "")).status, 405);
    assert_eq!(
        a.serve(&req("GET", "/subjects/%zz/versions", "")).status,
        400
    );
}
