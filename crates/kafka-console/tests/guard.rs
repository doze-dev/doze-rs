//! The guard, the same-origin proof, the body limit and the security headers: in front of every route (SEC-243 to SEC-245).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use axum::http::{StatusCode, header};
use kafka_console::routes::{Method, ROUTES};
use support::{HOST, app, get, post, send};

/// A path that fills the parameters of a pattern with values that are valid for it.
fn concrete(pattern: &str, state: &kafka_console::state::State) -> String {
    let asset = state.assets.urls.css.rsplit('/').next().unwrap().to_owned();
    pattern
        .replace("{id}", "home-instance")
        .replace("{name}", &asset)
        .replace("{topic}", "orders")
}

#[tokio::test]
async fn a_foreign_host_is_403_on_every_route_with_words_that_name_the_host_and_the_flag() {
    let (app, state) = app("/", &[]);
    for r in ROUTES {
        let path = concrete(r.path, &state);
        for host in [
            "evil.example",
            "127.0.0.1.evil.example",
            "localhost.evil.example:9093",
            "",
            "a@localhost",
        ] {
            let mut req = match r.method {
                Method::Get => get(&path),
                Method::Post => post(&path, "back=home"),
            };
            req.headers_mut()
                .insert(header::HOST, host.parse().unwrap());
            let (res, body) = send(&app, req).await;
            assert_eq!(
                res.status(),
                StatusCode::FORBIDDEN,
                "{} with Host {host:?}",
                r.path
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
    let path = "/connect/advertised/check";
    let bare = |origin: Option<&str>, site: Option<&str>| {
        let mut b = axum::http::Request::builder()
            .method("POST")
            .uri(path)
            .header(header::HOST, HOST);
        if let Some(o) = origin {
            b = b.header(header::ORIGIN, o);
        }
        if let Some(s) = site {
            b = b.header("sec-fetch-site", s);
        }
        b.body(axum::body::Body::from("back=home")).unwrap()
    };
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
        (Some("http://127.0.0.1:9093"), None, StatusCode::SEE_OTHER),
        (None, Some("same-origin"), StatusCode::SEE_OTHER),
        (
            Some("http://127.0.0.1:9093"),
            Some("same-origin"),
            StatusCode::SEE_OTHER,
        ),
    ] {
        let (res, body) = send(&app, bare(origin, site)).await;
        assert_eq!(res.status(), want, "{origin:?} {site:?}: {body:.200}");
    }
    // Every GET row is a read: none of them may change the broker, which is what `mutates` says.
    assert!(
        ROUTES
            .iter()
            .filter(|r| r.method == Method::Get)
            .all(|r| !r.mutates)
    );
}

#[tokio::test]
async fn a_body_over_a_mebibyte_is_refused_unread() {
    let (app, _) = app("/", &[]);
    let big = "a".repeat((1 << 20) + 1);
    let (res, _) = send(
        &app,
        post("/connect/advertised/check", &format!("back={big}")),
    )
    .await;
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn every_answer_carries_the_security_headers() {
    let (app, state) = app("/", &[]);
    for path in [
        "/",
        "/connect",
        "/nope",
        "/healthz",
        "/fragments/home-instance",
        state.assets.urls.css.as_str(),
    ] {
        let (res, _) = send(&app, get(path)).await;
        let h = res.headers();
        let csp = h[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
        assert!(
            csp.contains("default-src 'self'")
                && csp.contains("style-src 'self' 'unsafe-inline'")
                && csp.contains("frame-ancestors 'none'"),
            "{path}: {csp}"
        );
        assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff", "{path}");
        assert_eq!(h[header::REFERRER_POLICY], "no-referrer", "{path}");
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
    let (app, state) = app("/", &[]);
    let posts: Vec<_> = ROUTES.iter().filter(|r| r.method == Method::Post).collect();
    assert!(
        posts.len() >= 6,
        "the table has the writes of the topic pages: {}",
        posts.len()
    );
    for r in posts {
        let path = r.path.replace("{topic}", "orders");
        let bare = |origin: Option<&str>, site: Option<&str>| {
            let mut b = axum::http::Request::builder()
                .method("POST")
                .uri(&path)
                .header(header::HOST, HOST);
            if let Some(o) = origin {
                b = b.header(header::ORIGIN, o);
            }
            if let Some(s) = site {
                b = b.header("sec-fetch-site", s);
            }
            b.body(axum::body::Body::from(
                "confirm=orders&name=x&count=9&offset=1&partition=0&value=v",
            ))
            .unwrap()
        };
        for (origin, site) in [
            (None, None),
            (Some("http://evil.example"), None),
            (Some("null"), None),
            (Some("http://127.0.0.1:9093"), Some("cross-site")),
        ] {
            let (res, body) = send(&app, bare(origin, site)).await;
            assert_eq!(
                res.status(),
                StatusCode::FORBIDDEN,
                "POST {} {origin:?} {site:?}: {body:.200}",
                r.path
            );
        }
        // Every POST row is a mutation, and every mutation is a POST.
        assert!(r.mutates || r.op == Some("Metadata"), "{}", r.path);
    }
    assert!(
        ROUTES
            .iter()
            .filter(|r| r.mutates)
            .all(|r| r.method == Method::Post)
    );
    let _ = state;
}
