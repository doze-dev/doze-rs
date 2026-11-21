//! What keeps a console with no login safe to leave running: the `Host` guard, the proof the API asks for, the same-origin proof
//! in front of anything that is not a read, the headers on every answer (the refusals included), and files that come from the
//! binary and nowhere else.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use aws_console::headers::CSP;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use support::{HOST, app, get, header_of, json, send, state};

fn with_host(path: &str, host: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .header(header::HOST, host)
        .header("x-console", "1")
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn a_host_that_is_not_this_machine_is_refused_before_anything_is_answered() {
    let s = state(8, &[]);
    let app = app(&s);
    for host in [
        "evil.example",
        "evil.example:4567",
        "localhost.evil.example",
        "127.0.0.1.evil.example",
        "localhost:pw@evil.example",
        "",
    ] {
        for path in ["/", "/api/v1/about", "/assets/x.js", "/healthz"] {
            let (res, text) = send(&app, with_host(path, host)).await;
            assert_eq!(res.status(), StatusCode::FORBIDDEN, "{host} {path}");
            assert!(text.contains("This Host is not allowed"), "{host}: {text}");
            assert!(!text.contains("\"version\""), "{host}: {text}");
            assert_eq!(header_of(&res, "content-security-policy"), CSP, "{host}");
        }
    }
    for host in [
        "127.0.0.1:4567",
        "localhost:4567",
        "[::1]:4567",
        "app.localhost",
        "192.168.1.20:4567",
    ] {
        let (res, _) = send(&app, with_host("/api/v1/about", host)).await;
        assert_eq!(res.status(), StatusCode::OK, "{host}");
    }
}

#[tokio::test]
async fn a_refused_host_is_shown_as_text_and_a_listed_name_is_answered() {
    let s = state(8, &[]);
    let (res, text) = send(&app(&s), with_host("/", "<script>alert(1)</script>")).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert!(!text.contains("<script>"), "{text}");
    let listed = state(8, &["dev.internal"]);
    let app = app(&listed);
    let (res, _) = send(&app, with_host("/api/v1/about", "dev.internal:4567")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let (res, _) = send(&app, with_host("/api/v1/about", "other.internal:4567")).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn every_answer_carries_the_security_headers() {
    let s = state(8, &[]);
    let app = app(&s);
    let post = Request::builder()
        .method("POST")
        .uri("/")
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap();
    let no_proof = Request::builder()
        .uri("/api/v1/about")
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap();
    for (what, req, status) in [
        ("the page", get("/"), 200),
        ("a deep link", get("/live/calls/7"), 200),
        ("the API", get("/api/v1/about"), 200),
        ("an API address there is none of", get("/api/v1/nope"), 404),
        ("a file there is none of", get("/assets/nope.js"), 404),
        ("the health check", get("/healthz"), 200),
        ("a post to a page", post, 405),
        ("the API without its proof", no_proof, 403),
        ("another host", with_host("/", "evil.example"), 403),
    ] {
        let (res, _) = send(&app, req).await;
        assert_eq!(res.status().as_u16(), status, "{what}");
        assert_eq!(header_of(&res, "content-security-policy"), CSP, "{what}");
        assert_eq!(
            header_of(&res, "x-content-type-options"),
            "nosniff",
            "{what}"
        );
        assert_eq!(header_of(&res, "referrer-policy"), "same-origin", "{what}");
        assert_eq!(header_of(&res, "x-frame-options"), "DENY", "{what}");
        assert!(!header_of(&res, "cache-control").is_empty(), "{what}");
        assert_eq!(
            header_of(&res, "access-control-allow-origin"),
            "",
            "{what}: no CORS is ever granted"
        );
    }
    assert!(CSP.starts_with("default-src 'self';"), "{CSP}");
    assert!(CSP.contains("frame-ancestors 'none'"), "{CSP}");
    assert!(!CSP.contains("unsafe-eval"), "{CSP}");
    assert!(
        !CSP.contains("script-src"),
        "scripts fall to default-src 'self': {CSP}"
    );
}

#[tokio::test]
async fn an_api_answer_is_not_stored_and_the_api_asks_for_its_proof_header() {
    let s = state(8, &[]);
    let app = app(&s);
    let (res, _) = send(&app, get("/api/v1/about")).await;
    assert_eq!(header_of(&res, "cache-control"), "no-store");
    for path in [
        "/api/v1/about",
        "/api/v1/services",
        "/api/v1/calls",
        "/api/v1/tail/live",
    ] {
        let bare = Request::builder()
            .uri(path)
            .header(header::HOST, HOST)
            .body(Body::empty())
            .unwrap();
        let (status, body) = json(&app, bare).await;
        assert_eq!(status, 403, "{path}");
        assert_eq!(body["code"], "FORBIDDEN", "{path}");
        assert!(
            body["message"].as_str().unwrap().contains("X-Console"),
            "{body}"
        );
    }
    // A read another site's page made the browser send is refused although it carries the header.
    for site in ["cross-site", "same-site", "none"] {
        let mut req = get("/api/v1/about");
        req.headers_mut()
            .insert("sec-fetch-site", site.parse().unwrap());
        let (status, body) = json(&app, req).await;
        assert_eq!(status, 403, "{site}: {body}");
    }
    let mut own = get("/api/v1/about");
    own.headers_mut()
        .insert("sec-fetch-site", "same-origin".parse().unwrap());
    assert_eq!(json(&app, own).await.0, 200);
}

/// A request that is not a read, to an API address, with the proof header and what the browser said of where it came from.
fn write(origin: Option<&str>, site: Option<&str>) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri("/api/v1/about")
        .header(header::HOST, HOST)
        .header("x-console", "1")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(origin) = origin {
        req = req.header(header::ORIGIN, origin);
    }
    if let Some(site) = site {
        req = req.header("sec-fetch-site", site);
    }
    req.body(Body::from("{}")).unwrap()
}

#[tokio::test]
async fn a_write_must_prove_it_came_from_the_console_s_own_page() {
    let s = state(8, &[]);
    let app = app(&s);
    for (what, origin, site) in [
        ("no proof at all", None, None),
        ("another origin", Some("http://evil.example"), None),
        (
            "another port of this machine (the gateway's: an object in local S3)",
            Some("http://127.0.0.1:4566"),
            None,
        ),
        ("a null origin", Some("null"), None),
        (
            "an origin with a path",
            Some("http://127.0.0.1:4567/"),
            None,
        ),
        (
            "cross-site, whatever the origin says",
            Some("http://127.0.0.1:4567"),
            Some("cross-site"),
        ),
        ("same-site is not same-origin", None, Some("same-site")),
    ] {
        let (status, body) = json(&app, write(origin, site)).await;
        assert_eq!(status, 403, "{what}: {body}");
        assert_eq!(body["code"], "FORBIDDEN", "{what}");
    }
    // With the proof it passes the guard, and is then told that nothing here takes a write yet.
    for (origin, site) in [
        (Some("http://127.0.0.1:4567"), None),
        (None, Some("same-origin")),
    ] {
        let (res, _) = send(&app, write(origin, site)).await;
        assert_eq!(
            res.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{origin:?} {site:?}"
        );
    }
}

#[tokio::test]
async fn a_body_over_the_limit_is_refused_unread() {
    let s = state(8, &[]);
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/about")
        .header(header::HOST, HOST)
        .header("x-console", "1")
        .header(header::ORIGIN, format!("http://{HOST}"))
        .header(header::CONTENT_LENGTH, (2 << 20).to_string())
        .body(Body::empty())
        .unwrap();
    let (res, text) = send(&app(&s), req).await;
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE, "{text}");
}
