//! The five security headers (SEC-245), with their whole values, on every kind of answer the router gives (audit A4), and the
//! body limit that answers before anything is read (audit A5); the bundle's files: ETag, 304, no range, gzip negotiation.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use kafka_console::state::State;
use support::{HOST, app, get, head, send};

/// The policy, written out in full here and not read from the code under test.
const CSP: &str = "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

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

/// A file of the bundle whose name ends with `suffix` (`.js`, `.css`, `.woff2`), as its address.
fn bundle_file(state: &State, suffix: &str) -> String {
    let mut paths: Vec<_> = state
        .bundle
        .paths()
        .filter(|p| p.starts_with("assets/") && p.ends_with(suffix))
        .collect();
    paths.sort_unstable();
    format!("/{}", paths.first().unwrap())
}

/// A request the React app's fetch client would send.
fn api(method: Method, path: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST)
        .header("x-console", "1")
        .header("sec-fetch-site", "same-origin")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body.to_owned()))
        .unwrap()
}

#[tokio::test]
async fn every_kind_of_answer_carries_all_five_headers_with_their_whole_values() {
    let (app, state) = app("/", &[]);
    // 200: a page (index.html), JSON, a file, the health check.
    for path in ["/", "/connect", "/healthz", &bundle_file(&state, ".css")] {
        let (res, _) = send(&app, get(path)).await;
        assert_eq!(res.status(), StatusCode::OK, "{path}");
        assert_all_five(res.headers(), path);
    }
    let (res, _) = send(&app, api(Method::GET, "/api/v1/home", "")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_all_five(res.headers(), "json");
    // 404: an API address that is not one, a file of the bundle that is not one.
    let (res, _) = send(&app, api(Method::GET, "/api/v1/nope", "")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_all_five(res.headers(), "404 of the API");
    let (res, _) = send(&app, get("/assets/nope.js")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_all_five(res.headers(), "404 of a file");
    // 405: a method the path does not take, with the proof a change needs.
    let (res, _) = send(&app, api(Method::PUT, "/", "")).await;
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
        .uri("/api/v1/probe")
        .header(header::HOST, HOST)
        .header("x-console", "1")
        .body(Body::empty())
        .unwrap();
    let (res, _) = send(&app, bare).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_all_five(res.headers(), "403 origin");
    // 400: a form that goes nowhere.
    let (res, _) = send(&app, api(Method::POST, "/api/v1/lab/rules/x/remove", "")).await;
    assert!(res.status().is_client_error(), "{}", res.status());
    assert_all_five(res.headers(), "4xx");
    // 413: declared over the limit, and undeclared (cut at the limit).
    let mut declared = api(Method::POST, "/api/v1/probe", "");
    declared
        .headers_mut()
        .insert(header::CONTENT_LENGTH, "1048577".parse().unwrap());
    let (res, _) = send(&app, declared).await;
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_all_five(res.headers(), "413 declared");
    let big = "a".repeat((1 << 20) + 1);
    let (res, _) = send(
        &app,
        api(Method::POST, "/api/v1/create-topic", &format!("name={big}")),
    )
    .await;
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_all_five(res.headers(), "413 undeclared");
    // The head of a stream, which has no end.
    let res = head(&app, api(Method::GET, "/api/v1/tail/live", "")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_all_five(res.headers(), "stream head");
}

#[tokio::test]
async fn a_cache_rule_a_handler_set_is_kept_and_any_other_answer_is_not_stored() {
    let (app, state) = app("/", &[]);
    let (res, _) = send(&app, get("/")).await;
    assert_eq!(res.headers()[header::CACHE_CONTROL], "no-cache");
    let (res, _) = send(&app, get(&bundle_file(&state, ".css"))).await;
    assert_eq!(
        res.headers()[header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );
    let (res, _) = send(&app, api(Method::GET, "/api/v1/home", "")).await;
    assert_eq!(res.headers()[header::CACHE_CONTROL], "no-store");
    let (res, _) = send(&app, get("/assets/nope.js")).await;
    assert_eq!(res.headers()[header::CACHE_CONTROL], "no-store");
}

#[tokio::test]
async fn a_body_that_says_it_is_over_a_mebibyte_is_refused_before_it_is_read_on_every_kind_of_route()
 {
    let (app, _) = app("/", &[]);
    // A route that reads a body: the declared length alone refuses it, though the body that follows is tiny.
    let mut declared = api(Method::POST, "/api/v1/create-topic", "name=x");
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
    // Up to the limit is read.
    let (res, _) = send(&app, api(Method::POST, "/api/v1/probe", "")).await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_static_file_answers_304_to_its_own_etag_and_ignores_a_range() {
    let (app, state) = app("/", &[]);
    let css = bundle_file(&state, ".css");
    let (res, body) = send(&app, get(&css)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let etag = res.headers()[header::ETAG].to_str().unwrap().to_owned();
    assert!(etag.starts_with('"') && etag.contains("assets/"), "{etag}");
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
        let mut r = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/probe")
            .header(header::HOST, HOST)
            .header("x-console", "1")
            .header(header::ORIGIN, origin)
            .body(Body::empty())
            .unwrap();
        r.headers_mut()
            .append(header::HOST, "evil.example".parse().unwrap());
        r
    };
    let (res, _) = send(&app, two(&format!("http://{HOST}"))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let (res, _) = send(&app, two("http://evil.example")).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    // And the allowed host cannot be the second one.
    let mut r = get("/");
    r.headers_mut()
        .insert(header::HOST, "evil.example".parse().unwrap());
    r.headers_mut().append(header::HOST, HOST.parse().unwrap());
    assert_eq!(send(&app, r).await.0.status(), StatusCode::FORBIDDEN);
}

/// The answer to a GET of a file with this `Accept-Encoding` (none when `None`): headers and raw body bytes.
async fn raw_get(app: &axum::Router, path: &str, accept: Option<&str>) -> (HeaderMap, Vec<u8>) {
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let mut req = get(path);
    if let Some(a) = accept {
        req.headers_mut()
            .insert(header::ACCEPT_ENCODING, a.parse().unwrap());
    }
    let res = app.clone().oneshot(req).await.unwrap();
    let (parts, body) = res.into_parts();
    (
        parts.headers,
        body.collect().await.unwrap().to_bytes().to_vec(),
    )
}

/// A `Vary` names `Accept-Encoding`.
fn varies_on_encoding(h: &HeaderMap) -> bool {
    h.get_all(header::VARY)
        .iter()
        .any(|v| v.to_str().unwrap().contains("Accept-Encoding"))
}

fn inflate(gz: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(gz)
        .read_to_end(&mut out)
        .unwrap();
    out
}

#[tokio::test]
async fn text_assets_are_sent_as_gzip_to_a_client_that_accepts_it_and_inflated_to_one_that_does_not()
 {
    let (app, state) = app("/", &[]);
    let files = [bundle_file(&state, ".css"), bundle_file(&state, ".js")];
    for path in &files {
        // No Accept-Encoding, an unrelated one, and gzip refused with q=0: the file itself, under its own tag.
        let mut plain_tag = None;
        for accept in [None, Some("identity"), Some("br, gzip;q=0")] {
            let (h, body) = raw_get(&app, path, accept).await;
            assert!(
                h.get(header::CONTENT_ENCODING).is_none(),
                "{path} {accept:?}"
            );
            assert!(varies_on_encoding(&h), "{path} {accept:?}");
            assert_ne!(&body[..2], &[0x1f, 0x8b], "{path} {accept:?}: still gzip");
            assert!(body.len() > 1000);
            plain_tag = Some(h[header::ETAG].to_str().unwrap().to_owned());
        }
        // gzip accepted: the embedded bytes as they are, which inflate to the same file, under another tag.
        let plain = raw_get(&app, path, None).await.1;
        for accept in ["gzip", "gzip, deflate", "br;q=1.0, gzip;q=0.5", "*", "GZIP"] {
            let (h, body) = raw_get(&app, path, Some(accept)).await;
            assert_eq!(h[header::CONTENT_ENCODING], "gzip", "{path} {accept}");
            assert!(varies_on_encoding(&h), "{path} {accept:?}");
            assert!(
                body.len() < plain.len() / 2,
                "{path}: {} against {}",
                body.len(),
                plain.len()
            );
            assert_eq!(inflate(&body), plain, "{path} {accept}");
            assert_ne!(
                Some(h[header::ETAG].to_str().unwrap().to_owned()),
                plain_tag
            );
        }
    }
    // A font is already compressed: never gzip, never Vary.
    let (h, _) = raw_get(&app, &bundle_file(&state, ".woff2"), Some("gzip")).await;
    assert!(h.get(header::CONTENT_ENCODING).is_none());
    assert!(!varies_on_encoding(&h));
}
