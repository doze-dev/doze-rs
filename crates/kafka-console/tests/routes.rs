//! The route table is the router; the guard and the origin proof are in front of every route; every link resolves.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::collections::BTreeSet;

use axum::http::{StatusCode, header};
use kafka_console::routes::{Handler, Method, ROUTES};
use support::{HOST, app, get, local_links, post, send};

/// A path that fills the parameters of a pattern with values that are valid for it.
fn concrete(pattern: &str, state: &kafka_console::state::State) -> String {
    let asset = state.assets.urls.css.rsplit('/').next().unwrap().to_owned();
    pattern
        .replace("{id}", "home-instance")
        .replace("{name}", &asset)
        .replace("{topic}", "orders")
}

#[tokio::test]
async fn every_row_of_the_table_has_a_handler_and_every_handler_a_row() {
    let (app, state) = app("/", &[]);
    for r in ROUTES {
        let path = concrete(r.path, &state);
        let req = match r.method {
            Method::Get => get(&path),
            Method::Post => post(&path, "back=home"),
        };
        // The stream never ends: its answer is judged by its head alone.
        let (res, body) = if r.handler == Handler::Events {
            (support::head(&app, req).await, String::new())
        } else {
            send(&app, req).await
        };
        assert!(
            res.status().is_success() || res.status().is_redirection(),
            "{} {} answered {} {body:.200}",
            if r.method == Method::Get {
                "GET"
            } else {
                "POST"
            },
            r.path,
            res.status()
        );
    }
    // And what is not a row is not answered: the router is built from the table and from nothing else.
    for path in [
        "/nope",
        "/topics/orders",
        "/connect/advertised/",
        "/static",
        "/fragments",
        "/events/x",
        "/healthz/x",
        "/api",
    ] {
        let (res, _) = send(&app, get(path)).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn an_unknown_route_is_a_404_page_in_the_shell_and_a_wrong_method_a_405() {
    let (app, _) = app("/", &[]);
    let (res, body) = send(&app, get("/no/such/page")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert!(
        body.contains("<nav class=\"rail\"") && body.contains("There is no page at"),
        "{body:.300}"
    );
    assert!(body.contains("/no/such/page"));
    let (res, body) = send(&app, post("/connect", "")).await;
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert!(body.contains("<nav class=\"rail\""), "{body:.200}");
    let (res, _) = send(&app, get("/connect/advertised/check")).await;
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
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
async fn the_check_form_comes_back_to_its_page_without_a_script_and_swaps_a_fragment_with_one() {
    let (app, _) = app("/k", &[]);
    for (back, to) in [("home", "/k/"), ("advertised", "/k/connect/advertised")] {
        let (res, _) = send(
            &app,
            post("/k/connect/advertised/check", &format!("back={back}")),
        )
        .await;
        assert_eq!(res.status(), StatusCode::SEE_OTHER);
        assert_eq!(res.headers()[header::LOCATION], to);
    }
    let mut req = post("/k/connect/advertised/check", "back=home");
    req.headers_mut()
        .insert("hx-request", "true".parse().unwrap());
    let (res, body) = send(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        body.starts_with("<div class=\"frag\" id=\"probe-home\""),
        "{body:.200}"
    );
    let (res, _) = send(&app, post("/k/connect/advertised/check", "back=elsewhere")).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let (res, _) = send(&app, post("/k/connect/advertised/check", "")).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
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
async fn every_href_and_src_of_every_page_resolves_under_the_base() {
    for prefix in ["/", "/kafka"] {
        let (app, state) = app(prefix, &[]);
        let mut seen = BTreeSet::new();
        let mut queue: Vec<String> = [
            "/",
            "/?state=quiet",
            "/?state=attention",
            "/connect",
            "/connect/advertised",
        ]
        .iter()
        .map(|p| state.base.url(p))
        .collect();
        while let Some(path) = queue.pop() {
            if !seen.insert(path.clone()) {
                continue;
            }
            let (res, body) = send(&app, get(&path)).await;
            assert_eq!(res.status(), StatusCode::OK, "{path}");
            if res.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/html")
            {
                for link in local_links(&body) {
                    let want = state.base.url("/");
                    assert!(
                        link.starts_with(&want) || link == state.base.prefix(),
                        "{path} links to {link}, which is outside the base {want}"
                    );
                    queue.push(link);
                }
            }
        }
        assert!(
            seen.len() >= 12,
            "{prefix}: only {} urls crawled: {seen:?}",
            seen.len()
        );
        assert!(
            seen.iter()
                .any(|u| u.contains("/static/console.") && u.ends_with(".css"))
        );
        assert!(
            seen.iter()
                .any(|u| u.contains("/static/htmx.min.") && u.ends_with(".js"))
        );
        assert!(
            seen.iter()
                .any(|u| u.contains("/static/console.") && u.ends_with(".js"))
        );
        assert!(seen.iter().any(|u| u.contains("/static/favicon.")));
    }
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
async fn pages_are_not_stored_fragments_revalidate_and_files_are_kept_for_a_year() {
    let (app, state) = app("/", &[]);
    assert_eq!(
        send(&app, get("/")).await.0.headers()[header::CACHE_CONTROL],
        "no-store"
    );
    let (res, body) = send(&app, get("/fragments/home-instance")).await;
    assert_eq!(res.headers()[header::CACHE_CONTROL], "no-cache");
    let etag = res.headers()[header::ETAG].to_str().unwrap().to_owned();
    assert!(body.contains("This instance"));
    let mut again = get("/fragments/home-instance");
    again
        .headers_mut()
        .insert(header::IF_NONE_MATCH, etag.parse().unwrap());
    let (res, body) = send(&app, again).await;
    assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
    assert!(body.is_empty());
    let mut stale = get("/fragments/home-instance");
    stale
        .headers_mut()
        .insert(header::IF_NONE_MATCH, "\"other\"".parse().unwrap());
    assert_eq!(send(&app, stale).await.0.status(), StatusCode::OK);
    let (res, _) = send(&app, get(&state.assets.urls.css)).await;
    assert_eq!(
        res.headers()[header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );
    assert_eq!(
        res.headers()[header::CONTENT_TYPE],
        "text/css; charset=utf-8"
    );
    let (res, _) = send(&app, get("/static/console.css")).await;
    assert_eq!(
        res.status(),
        StatusCode::NOT_FOUND,
        "an unhashed name is not a file"
    );
}

#[tokio::test]
async fn the_stream_takes_only_the_closed_vocabulary_and_an_unknown_fragment_is_a_404() {
    let (app, _) = app("/", &[]);
    let (res, body) = send(&app, get("/events?watch=home-instance,evil")).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert!(
        body.contains("\"evil\" is not a fragment this console can watch"),
        "{body}"
    );
    let (res, _) = send(&app, get("/fragments/etc-passwd")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let (res, body) = send(&app, get("/healthz")).await;
    assert_eq!((res.status(), body.as_str()), (StatusCode::OK, "ok\n"));
}
