//! The five security headers (SEC-245), with their whole values, on every kind of answer the router gives (audit A4), and the
//! body limit that answers before anything is read (audit A5).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use support::{HOST, app, get, head, post, send};

/// The policy, written out in full here and not read from the code under test.
const CSP: &str = "default-src 'self'; style-src 'self' 'unsafe-inline'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

fn assert_all_five(h: &HeaderMap, what: &str) {
    assert_eq!(h[header::CONTENT_SECURITY_POLICY], CSP, "{what}: CSP");
    assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff", "{what}");
    assert_eq!(h[header::REFERRER_POLICY], "same-origin", "{what}");
    assert_eq!(h[header::X_FRAME_OPTIONS], "DENY", "{what}");
    assert!(
        h.contains_key(header::CACHE_CONTROL),
        "{what}: no cache rule at all"
    );
}

#[tokio::test]
async fn every_kind_of_answer_carries_all_five_headers_with_their_whole_values() {
    let (app, state) = app("/", &[]);
    // 200: a page, a fragment, a file, the health check.
    for path in [
        "/",
        "/connect",
        "/healthz",
        "/fragments/home-live",
        state.assets.urls.css.as_str(),
    ] {
        let (res, _) = send(&app, get(path)).await;
        assert_eq!(res.status(), StatusCode::OK, "{path}");
        assert_all_five(res.headers(), path);
    }
    // 404, in the shell and without.
    let (res, _) = send(&app, get("/no/such/page")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_all_five(res.headers(), "404");
    let (res, _) = send(&app, get("/fragments/nope")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_all_five(res.headers(), "404 of a fragment");
    // 405: a method the path does not take, with the proof a change needs.
    let mut put = Request::builder()
        .method(Method::PUT)
        .uri("/")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("http://{HOST}"))
        .body(Body::empty())
        .unwrap();
    put.headers_mut()
        .insert("sec-fetch-site", "same-origin".parse().unwrap());
    let (res, _) = send(&app, put).await;
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_all_five(res.headers(), "405");
    // 403: a Host that is not allowed, and a change with no proof.
    let mut foreign = get("/");
    foreign
        .headers_mut()
        .insert(header::HOST, "evil.example".parse().unwrap());
    let (res, _) = send(&app, foreign).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_all_five(res.headers(), "403 host");
    let bare = Request::builder()
        .method(Method::POST)
        .uri("/connect/advertised/check")
        .header(header::HOST, HOST)
        .body(Body::from("back=home"))
        .unwrap();
    let (res, _) = send(&app, bare).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_all_five(res.headers(), "403 origin");
    // 400: a stream that watches what it may not, and a form that goes nowhere.
    let (res, _) = send(&app, get("/events?watch=nope")).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_all_five(res.headers(), "400");
    // 413: declared over the limit, and undeclared (cut at the limit).
    let mut declared = post("/connect/advertised/check", "back=home");
    declared
        .headers_mut()
        .insert(header::CONTENT_LENGTH, "1048577".parse().unwrap());
    let (res, _) = send(&app, declared).await;
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_all_five(res.headers(), "413 declared");
    let big = "a".repeat((1 << 20) + 1);
    let (res, _) = send(
        &app,
        post("/connect/advertised/check", &format!("back={big}")),
    )
    .await;
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_all_five(res.headers(), "413 undeclared");
    // 303: the redirect of the check form.
    let (res, _) = send(&app, post("/connect/advertised/check", "back=home")).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_all_five(res.headers(), "303");
    // The head of a stream, which has no end.
    let res = head(&app, get("/events?watch=home-live")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_all_five(res.headers(), "stream head");
}

#[tokio::test]
async fn a_cache_rule_a_handler_set_is_kept_and_any_other_answer_is_not_stored() {
    let (app, state) = app("/", &[]);
    let (res, _) = send(&app, get("/")).await;
    assert_eq!(res.headers()[header::CACHE_CONTROL], "no-store");
    let (res, _) = send(&app, get("/fragments/home-live")).await;
    assert_eq!(res.headers()[header::CACHE_CONTROL], "no-cache");
    let (res, _) = send(&app, get(&state.assets.urls.css)).await;
    assert_eq!(
        res.headers()[header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );
    let (res, _) = send(&app, get("/nope")).await;
    assert_eq!(res.headers()[header::CACHE_CONTROL], "no-store");
}

#[tokio::test]
async fn a_body_that_says_it_is_over_a_mebibyte_is_refused_before_it_is_read_on_every_kind_of_route()
 {
    let (app, _) = app("/", &[]);
    // A route that reads a body: the declared length alone refuses it, though the body that follows is tiny.
    let mut declared = post("/connect/advertised/check", "back=home");
    declared
        .headers_mut()
        .insert(header::CONTENT_LENGTH, "1048577".parse().unwrap());
    let (res, body) = send(&app, declared).await;
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(body.contains("The console accepts at most 1 MiB"), "{body}");
    // A route that never reads one is refused all the same: the layer does not wait for a handler to ask.
    let mut get_with_body = get("/");
    get_with_body
        .headers_mut()
        .insert(header::CONTENT_LENGTH, "2000000".parse().unwrap());
    let (res, body) = send(&app, get_with_body).await;
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(body.contains("That is too large"), "{body}");
    // Exactly the limit is allowed.
    let mut exact = post("/connect/advertised/check", "back=home");
    exact.headers_mut().insert(
        header::CONTENT_LENGTH,
        (1 << 20).to_string().parse().unwrap(),
    );
    let (res, _) = send(&app, exact).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn a_static_file_answers_304_to_its_own_etag_and_ignores_a_range() {
    let (app, state) = app("/", &[]);
    let css = state.assets.urls.css.clone();
    let (res, body) = send(&app, get(&css)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let etag = res.headers()[header::ETAG].to_str().unwrap().to_owned();
    assert!(etag.starts_with('"') && etag.contains("console."), "{etag}");
    assert_eq!(res.headers()[header::ACCEPT_RANGES], "none");
    // The browser asks again with the tag: nothing is sent, and the rules for keeping it are repeated.
    let mut again = get(&css);
    again
        .headers_mut()
        .insert(header::IF_NONE_MATCH, etag.parse().unwrap());
    let (res, empty) = send(&app, again).await;
    assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
    assert!(empty.is_empty());
    assert_eq!(res.headers()[header::ETAG], etag.as_str());
    assert_eq!(
        res.headers()[header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );
    assert_all_five(res.headers(), "304 of a file");
    // Another tag, a weak form of the same, a list that includes it, and `*`.
    for (sent, want) in [
        ("\"other\"", StatusCode::OK),
        (format!("W/{etag}").leak() as &str, StatusCode::NOT_MODIFIED),
        (
            format!("\"a\", {etag}").leak() as &str,
            StatusCode::NOT_MODIFIED,
        ),
        ("*", StatusCode::NOT_MODIFIED),
    ] {
        let mut r = get(&css);
        r.headers_mut()
            .insert(header::IF_NONE_MATCH, sent.parse().unwrap());
        assert_eq!(send(&app, r).await.0.status(), want, "{sent}");
    }
    // A range is not honoured: the whole file, 200, and the answer says ranges are not offered.
    let mut ranged = get(&css);
    ranged
        .headers_mut()
        .insert(header::RANGE, "bytes=0-10".parse().unwrap());
    let (res, partial) = send(&app, ranged).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(partial, body);
    assert!(res.headers().get(header::CONTENT_RANGE).is_none());
    assert_eq!(res.headers()[header::ACCEPT_RANGES], "none");
}

#[tokio::test]
async fn with_two_host_headers_the_guard_and_the_origin_proof_read_the_same_first_one() {
    // Audit A25: not exploitable, and pinned: both layers use the first value, so they cannot disagree about which host
    // was asked. (A browser sends one; a request with two is not a browser's.)
    let (app, _) = app("/", &[]);
    let two = |origin: &str| {
        let mut r = post("/connect/advertised/check", "back=home");
        r.headers_mut()
            .insert(header::ORIGIN, origin.parse().unwrap());
        r.headers_mut()
            .append(header::HOST, "evil.example".parse().unwrap());
        r
    };
    let (res, _) = send(&app, two(&format!("http://{HOST}"))).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let (res, _) = send(&app, two("http://evil.example")).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    // And the allowed host cannot be the second one.
    let mut r = get("/");
    r.headers_mut()
        .insert(header::HOST, "evil.example".parse().unwrap());
    r.headers_mut().append(header::HOST, HOST.parse().unwrap());
    assert_eq!(send(&app, r).await.0.status(), StatusCode::FORBIDDEN);
}
