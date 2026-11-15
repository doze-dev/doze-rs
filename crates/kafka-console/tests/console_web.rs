//! The React console as the binary serves it (slice R0): the bundle, the client routing fallback, the JSON API and its
//! proof, and the layers in front of all of it. (The server-rendered console that lived under `/classic/` is gone, slice R5.)
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use kafka_console::app::console;
use kafka_console::base::Base;
use kafka_console::limits::Limits;
use kafka_console::source::Source;
use kafka_console::state::State;
use support::{HOST, send};
use tokio_util::sync::CancellationToken;

fn app(prefix: &str) -> Router {
    let state = State::console(
        Source::Fixture,
        &Base::new(prefix),
        vec![],
        CancellationToken::new(),
        &Limits::default(),
    );
    console(state)
}

fn req(method: &str, path: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST)
}

fn api_get(path: &str) -> Request<Body> {
    req("GET", path)
        .header("x-console", "1")
        .header("sec-fetch-site", "same-origin")
        .body(Body::empty())
        .unwrap()
}

fn plain_get(path: &str) -> Request<Body> {
    req("GET", path).body(Body::empty()).unwrap()
}

#[tokio::test]
async fn every_page_address_answers_index_html_so_a_deep_link_works() {
    let app = app("/");
    for path in [
        "/",
        "/connect",
        "/connect?as=properties",
        "/topics/orders",
        "/groups/a%2Fb",
        "/gallery?record=r7",
        "/no/such/page",
    ] {
        let (res, body) = send(&app, plain_get(path)).await;
        assert_eq!(res.status(), StatusCode::OK, "{path}");
        assert_eq!(
            res.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8",
            "{path}"
        );
        assert_eq!(res.headers()[header::CACHE_CONTROL], "no-cache", "{path}");
        assert!(body.contains("<div id=\"root\">"), "{path}");
        assert!(
            body.contains("<meta name=\"doze-base\" content=\"\">"),
            "{path}"
        );
        assert!(body.contains("src=\"/assets/"), "{path}: {body}");
    }
}

#[tokio::test]
async fn a_hashed_file_is_gzip_to_a_client_that_accepts_it_and_cached_for_a_year() {
    let app = app("/");
    let (_, index) = send(&app, plain_get("/")).await;
    let js = index
        .split("src=\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .unwrap()
        .to_owned();
    let gz = req("GET", &js)
        .header(header::ACCEPT_ENCODING, "gzip, deflate, br")
        .body(Body::empty())
        .unwrap();
    let (res, _) = send(&app, gz).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::CONTENT_ENCODING], "gzip");
    assert_eq!(
        res.headers()[header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );
    assert_eq!(
        res.headers()[header::CONTENT_TYPE],
        "text/javascript; charset=utf-8"
    );
    let (res, body) = send(&app, plain_get(&js)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(res.headers().get(header::CONTENT_ENCODING).is_none());
    assert!(body.len() > 10_000, "inflated for a client without gzip");
    let (res, _) = send(&app, plain_get("/assets/nothing-here.js")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let (res, _) = send(&app, plain_get("/favicon.svg")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::CACHE_CONTROL], "no-cache");
}

#[tokio::test]
async fn the_api_answers_json_only_to_the_consoles_own_fetch() {
    let app = app("/");
    for path in kafka_console::api::GET_PATHS {
        let (res, body) = send(&app, api_get(path)).await;
        assert_eq!(res.status(), StatusCode::OK, "{path}: {body}");
        assert_eq!(
            res.headers()[header::CONTENT_TYPE],
            "application/json; charset=utf-8",
            "{path}"
        );
        assert_eq!(res.headers()[header::CACHE_CONTROL], "no-store", "{path}");
        assert!(body.starts_with('{'), "{path}: {body}");
        // No memory figure anywhere (owner, 2026-11-13).
        assert!(!body.contains("resident"), "{path}: {body}");
        // Without the header: refused, in JSON.
        let (res, body) = send(&app, plain_get(path)).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "{path}");
        assert!(body.contains("\"code\":\"FORBIDDEN\""), "{body}");
        assert!(body.contains("X-Console: 1"), "{body}");
        // From another site, even with the header (a preflight would stop a browser; the server says no too).
        let cross = req("GET", path)
            .header("x-console", "1")
            .header("sec-fetch-site", "cross-site")
            .body(Body::empty())
            .unwrap();
        let (res, _) = send(&app, cross).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "{path}");
    }
}

#[tokio::test]
async fn the_api_says_what_home_connect_the_shell_and_the_palette_need() {
    let app = app("/");
    let (_, home) = send(&app, api_get("/api/v1/home")).await;
    let home: serde_json::Value = serde_json::from_str(&home).unwrap();
    assert_eq!(home["home"]["state"]["kind"], "steady");
    assert_eq!(home["words"]["title"], "Home");
    assert!(home["home"]["probe"]["rules"].as_array().is_some());
    let (_, shell) = send(&app, api_get("/api/v1/shell")).await;
    let shell: serde_json::Value = serde_json::from_str(&shell).unwrap();
    assert!(shell["address"].as_str().unwrap().contains(':'));
    assert!(shell["topics"].as_u64().unwrap() > 0);
    let (_, connect) = send(&app, api_get("/api/v1/connect")).await;
    let connect: serde_json::Value = serde_json::from_str(&connect).unwrap();
    assert!(
        connect["words"]["properties"]
            .as_str()
            .unwrap()
            .starts_with("bootstrap.servers=")
    );
    assert!(
        !connect["connect"]["snippets"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let (_, all) = send(&app, api_get("/api/v1/jump")).await;
    let all: serde_json::Value = serde_json::from_str(&all).unwrap();
    let (_, some) = send(&app, api_get("/api/v1/jump?q=ORD")).await;
    let some: serde_json::Value = serde_json::from_str(&some).unwrap();
    assert_eq!(some["query"], "ORD");
    let topics = some["topics"].as_array().unwrap();
    assert!(!topics.is_empty());
    assert!(
        topics
            .iter()
            .all(|t| t.as_str().unwrap().to_lowercase().contains("ord"))
    );
    assert!(some["total"].as_u64() < all["total"].as_u64());
}

#[tokio::test]
async fn a_change_through_the_api_needs_the_same_origin_proof_too() {
    let app = app("/");
    let bare = req("POST", "/api/v1/probe")
        .header("x-console", "1")
        .body(Body::empty())
        .unwrap();
    let (res, body) = send(&app, bare).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN, "{body}");
    let other = req("POST", "/api/v1/probe")
        .header("x-console", "1")
        .header(header::ORIGIN, "http://evil.example")
        .body(Body::empty())
        .unwrap();
    let (res, _) = send(&app, other).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let ok = req("POST", "/api/v1/probe")
        .header("x-console", "1")
        .header("sec-fetch-site", "same-origin")
        .body(Body::empty())
        .unwrap();
    let (res, body) = send(&app, ok).await;
    assert_eq!(res.status(), StatusCode::OK, "{body}");
    assert!(body.contains("\"verdict\""), "{body}");
    // A GET of a POST route, and an address the API does not have: JSON answers.
    let (res, _) = send(&app, api_get("/api/v1/probe")).await;
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    let (res, body) = send(&app, api_get("/api/v1/nothing")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert!(body.contains("\"code\":\"NOT_FOUND\""), "{body}");
    let post_page = req("POST", "/topics")
        .header("sec-fetch-site", "same-origin")
        .body(Body::empty())
        .unwrap();
    let (res, body) = send(&app, post_page).await;
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED, "{body}");
}

#[tokio::test]
async fn an_address_the_app_has_no_page_for_is_its_not_found_and_no_old_file_is_served() {
    let app = app("/");
    let (_, index) = send(&app, plain_get("/")).await;
    for path in [
        "/no-such-page",
        "/topics/orders/no-such-tab/deeper",
        "/events?tail=live",
    ] {
        // The React app's index.html, whose router has no such page and shows Not here.
        let (res, body) = send(&app, plain_get(path)).await;
        assert_eq!(res.status(), StatusCode::OK, "{path}");
        assert_eq!(body, index, "{path}");
    }
    for path in [
        "/static/console.0123456789ab.css",
        "/static/htmx-2.0.11.min.js",
    ] {
        let (_, body) = send(&app, plain_get(path)).await;
        assert_eq!(body, index, "no file outside the bundle is served: {path}");
    }
    let post = req("POST", "/connect/advertised/check")
        .header("sec-fetch-site", "same-origin")
        .body(Body::empty())
        .unwrap();
    let (res, _) = send(&app, post).await;
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    let (res, body) = send(&app, plain_get("/healthz")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body, "ok\n");
}

#[tokio::test]
async fn under_a_prefix_everything_moves_with_it() {
    let app = app("/kafka");
    let (res, body) = send(&app, plain_get("/kafka/topics/orders")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body.contains("content=\"/kafka\""), "{body}");
    assert!(body.contains("src=\"/kafka/assets/"), "{body}");
    let (res, body) = send(&app, api_get("/kafka/api/v1/shell")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body.contains("\"address\""), "{body}");
    let (res, _) = send(&app, plain_get("/kafka/healthz")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let (res, _) = send(&app, plain_get("/connect")).await;
    assert_eq!(
        res.status(),
        StatusCode::NOT_FOUND,
        "outside the prefix there is nothing"
    );
    let (res, _) = send(&app, plain_get("/elsewhere")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_host_guard_and_the_security_headers_stand_in_front_of_all_of_it() {
    let app = app("/");
    for path in ["/", "/topics", "/api/v1/home", "/healthz", "/favicon.svg"] {
        let evil = Request::builder()
            .uri(path)
            .header(header::HOST, "evil.example")
            .header("x-console", "1")
            .body(Body::empty())
            .unwrap();
        let (res, _) = send(&app, evil).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "{path}");
        let (res, _) = send(&app, api_get(path)).await;
        let csp = res.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap();
        assert_eq!(csp, kafka_console::headers::CSP, "{path}");
        assert!(csp.contains("connect-src 'self'") && !csp.contains("unsafe-eval"));
        assert_eq!(res.headers()[header::X_FRAME_OPTIONS], "DENY", "{path}");
        assert_eq!(
            res.headers()[header::X_CONTENT_TYPE_OPTIONS],
            "nosniff",
            "{path}"
        );
    }
}

#[tokio::test]
async fn a_hostile_name_in_the_palette_comes_back_as_json_text() {
    let app = app("/");
    let (res, body) = send(
        &app,
        api_get("/api/v1/jump?q=%3Cscript%3Ealert(1)%3C%2Fscript%3E"),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["query"], "<script>alert(1)</script>");
    assert_eq!(v["topics"].as_array().unwrap().len(), 0);
}
