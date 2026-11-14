//! The guard, the same-origin proof, the body limit and the security headers: in front of every route (SEC-243 to SEC-245).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use kafka_console::api::GET_PATHS;
use support::{HOST, app, get, send};

/// Every write of the API (a POST), with a body a form would send. The worlds of the tests have `orders` and `fulfilment`.
const POST_PATHS: &[&str] = &[
    "/api/v1/probe",
    "/api/v1/create-topic",
    "/api/v1/topics/orders/produce",
    "/api/v1/topics/orders/add-partitions",
    "/api/v1/topics/orders/delete-records",
    "/api/v1/topics/orders/delete",
    "/api/v1/topics/orders/config/edit",
    "/api/v1/topics/orders/config/reset",
    "/api/v1/topics/orders/log/compact",
    "/api/v1/groups/fulfilment/reset",
    "/api/v1/groups/fulfilment/delete",
    "/api/v1/groups/fulfilment/offsets/delete",
    "/api/v1/settings/edit",
    "/api/v1/access/acls/new",
    "/api/v1/access/acls/delete",
    "/api/v1/lab/rules",
    "/api/v1/lab/rules/1/enabled",
    "/api/v1/lab/rules/1/remove",
    "/api/v1/lab/history/clear",
    "/api/v1/lab/once/drop",
    "/api/v1/lab/compact",
];

/// What the React app's fetch client sends with a read.
fn api_get(path: &str) -> Request<Body> {
    let mut req = get(path);
    req.headers_mut().insert("x-console", "1".parse().unwrap());
    req
}

/// A write with the console's header, and the origin headers given.
fn api_post(path: &str, origin: Option<&str>, site: Option<&str>) -> Request<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::HOST, HOST)
        .header("x-console", "1")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    if let Some(o) = origin {
        b = b.header(header::ORIGIN, o);
    }
    if let Some(s) = site {
        b = b.header("sec-fetch-site", s);
    }
    b.body(Body::from(
        "confirm=orders&name=x&count=9&offset=1&partition=0&value=v",
    ))
    .unwrap()
}

#[tokio::test]
async fn a_foreign_host_is_403_on_every_route_with_words_that_name_the_host_and_the_flag() {
    let (app, _) = app("/", &[]);
    let pages = [
        "/",
        "/topics/orders",
        "/favicon.svg",
        "/healthz",
        "/api/v1/tail/live",
    ];
    let all = pages.iter().chain(GET_PATHS).chain(POST_PATHS);
    for path in all {
        for host in [
            "evil.example",
            "127.0.0.1.evil.example",
            "localhost.evil.example:9093",
            "",
            "a@localhost",
        ] {
            let mut req = if POST_PATHS.contains(path) {
                api_post(path, Some(&format!("http://{host}")), Some("same-origin"))
            } else {
                api_get(path)
            };
            req.headers_mut()
                .insert(header::HOST, host.parse().unwrap());
            let (res, body) = send(&app, req).await;
            assert_eq!(
                res.status(),
                StatusCode::FORBIDDEN,
                "{path} with Host {host:?}"
            );
            if !host.is_empty() {
                assert!(body.contains("--console-allow-host"), "{body}");
                assert!(body.contains("This Host is not allowed"), "{body}");
            }
        }
    }
}

#[tokio::test]
async fn an_allowed_host_is_served_and_a_hostile_one_is_escaped_in_the_refusal() {
    let (app, _) = app("/", &["dev.example.com"]);
    let mut req = get("/");
    req.headers_mut()
        .insert(header::HOST, "Dev.Example.com:9093".parse().unwrap());
    assert_eq!(send(&app, req).await.0.status(), StatusCode::OK);
    let mut req = get("/");
    req.headers_mut()
        .insert(header::HOST, "<script>alert(1)</script>".parse().unwrap());
    let (res, body) = send(&app, req).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert!(!body.contains("<script>alert"), "{body}");
    assert!(
        body.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
        "{body}"
    );
}

#[tokio::test]
async fn a_post_needs_a_same_origin_proof_and_a_get_changes_nothing() {
    let (app, _) = app("/", &[]);
    let path = "/api/v1/probe";
    for (origin, site, want) in [
        (None, None, StatusCode::FORBIDDEN),
        (Some("http://evil.example"), None, StatusCode::FORBIDDEN),
        (Some("null"), None, StatusCode::FORBIDDEN),
        (
            Some("http://127.0.0.1:9093"),
            Some("cross-site"),
            StatusCode::FORBIDDEN,
        ),
        (
            Some("http://127.0.0.1:9093"),
            Some("same-site"),
            StatusCode::FORBIDDEN,
        ),
        (Some("http://127.0.0.1:9093"), None, StatusCode::OK),
        (None, Some("same-origin"), StatusCode::OK),
        (
            Some("http://127.0.0.1:9093"),
            Some("same-origin"),
            StatusCode::OK,
        ),
    ] {
        let (res, body) = send(&app, api_post(path, origin, site)).await;
        assert_eq!(res.status(), want, "{origin:?} {site:?}: {body:.200}");
    }
    // A write is a POST: the same address read with GET is refused (or is the form's read, which changes nothing).
    let (res, _) = send(&app, api_get(path)).await;
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn a_body_over_a_mebibyte_is_refused_unread() {
    let (app, _) = app("/", &[]);
    let big = "a".repeat((1 << 20) + 1);
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/create-topic")
        .header(header::HOST, HOST)
        .header("x-console", "1")
        .header("sec-fetch-site", "same-origin")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("name={big}")))
        .unwrap();
    let (res, _) = send(&app, req).await;
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn every_answer_carries_the_security_headers() {
    let (app, _) = app("/", &[]);
    for path in [
        "/",
        "/connect",
        "/nope",
        "/healthz",
        "/favicon.svg",
        "/api/v1/home",
    ] {
        let (res, _) = send(&app, api_get(path)).await;
        let h = res.headers();
        let csp = h[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
        assert!(
            csp.contains("default-src 'self'")
                && csp.contains("style-src 'self' 'unsafe-inline'")
                && csp.contains("frame-ancestors 'none'"),
            "{path}: {csp}"
        );
        assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff", "{path}");
        assert_eq!(h[header::REFERRER_POLICY], "same-origin", "{path}");
        assert!(h.contains_key(header::CACHE_CONTROL), "{path}");
    }
    let mut refused = get("/");
    refused
        .headers_mut()
        .insert(header::HOST, "evil.example".parse().unwrap());
    assert!(
        send(&app, refused)
            .await
            .0
            .headers()
            .contains_key(header::CONTENT_SECURITY_POLICY),
        "a refusal has them too"
    );
}

#[tokio::test]
async fn every_post_row_needs_the_same_origin_proof_and_changes_nothing_without_it() {
    let (app, _) = app("/", &[]);
    for path in POST_PATHS {
        for (origin, site) in [
            (None, None),
            (Some("http://evil.example"), None),
            (Some("null"), None),
            (Some("http://127.0.0.1:9093"), Some("cross-site")),
        ] {
            let (res, body) = send(&app, api_post(path, origin, site)).await;
            assert_eq!(
                res.status(),
                StatusCode::FORBIDDEN,
                "POST {path} {origin:?} {site:?}: {body:.200}"
            );
        }
        // Without the console's own header, even from the console's own origin.
        let mut bare = api_post(path, None, Some("same-origin"));
        bare.headers_mut().remove("x-console");
        assert_eq!(
            send(&app, bare).await.0.status(),
            StatusCode::FORBIDDEN,
            "{path}"
        );
    }
    // The fallback takes no write: a POST to a page address is refused.
    let post_page = api_post("/topics/orders", None, Some("same-origin"));
    assert_eq!(
        send(&app, post_page).await.0.status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
}
