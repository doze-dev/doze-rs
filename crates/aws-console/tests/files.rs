//! The page and the files of the bundle, answered by the router: `index.html` for every address that is not the API or a file,
//! a hashed file as gzip and immutable, and no address that steps out of what is embedded.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::io::Write;

use aws_console::files::Served;
use aws_console::web::Bundle;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use support::{HOST, app, get, header_of, json, send, state};

#[tokio::test]
async fn the_page_is_the_embedded_index_for_every_address_that_is_not_the_api_or_a_file() {
    let s = state(8, &[]);
    let app = app(&s);
    for path in [
        "/",
        "/index.html",
        "/live",
        "/resources/sqs/orders",
        "/settings?tab=about",
    ] {
        let (res, text) = send(&app, get(path)).await;
        assert_eq!(res.status(), StatusCode::OK, "{path}");
        assert_eq!(
            header_of(&res, "content-type"),
            "text/html; charset=utf-8",
            "{path}"
        );
        assert_eq!(header_of(&res, "cache-control"), "no-cache", "{path}");
        assert!(
            text.contains("<title>doze-aws</title>") && text.contains("<div id=\"root\">"),
            "{path}: {text}"
        );
        assert!(
            text.contains("<meta name=\"doze-base\" content=\"\">"),
            "{path}: {text}"
        );
    }
    // The page asks for no proof header: a person types the address.
    let bare = Request::builder()
        .uri("/")
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&app, bare).await.0.status(), StatusCode::OK);
    let (status, body) = json(&app, get("/api/v1/nope")).await;
    assert_eq!((status, body["code"].as_str()), (404, Some("NOT_FOUND")));
    let (status, _) = json(&app, get("/api")).await;
    assert_eq!(status, 404, "the API's own root is not a page");
}

/// `text` as gzip, as the build script packs a file.
fn gz(text: &str) -> &'static [u8] {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    enc.write_all(text.as_bytes()).unwrap();
    Box::leak(enc.finish().unwrap().into_boxed_slice())
}

const SCRIPT: &str = "console.log('the doze-aws console');";

/// A bundle with a hashed script and a font, as a real build has.
fn built() -> Bundle {
    let files: &'static [(&'static str, Served)] = Box::leak(Box::new([
        (
            "index.html",
            Served {
                content_type: "text/html; charset=utf-8",
                bytes: gz(
                    "<html><head>\n<script type=\"module\" src=\"./assets/app-1a2b.js\"></script></head></html>",
                ),
                gzip: true,
            },
        ),
        (
            "assets/app-1a2b.js",
            Served {
                content_type: "text/javascript; charset=utf-8",
                bytes: gz(SCRIPT),
                gzip: true,
            },
        ),
        (
            "assets/font-9z.woff2",
            Served {
                content_type: "font/woff2",
                bytes: b"wOF2-not-really",
                gzip: false,
            },
        ),
    ]));
    Bundle::of(files)
}

#[tokio::test]
async fn a_hashed_file_is_served_gzip_and_immutable_with_a_tag() {
    let s = state(8, &[]);
    assert!(s.serve_bundle(built()));
    let app = app(&s);
    let mut req = get("/assets/app-1a2b.js");
    req.headers_mut().insert(
        header::ACCEPT_ENCODING,
        "gzip, deflate, br".parse().unwrap(),
    );
    let res = tower::ServiceExt::oneshot(app.clone(), req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(header_of(&res, "content-encoding"), "gzip");
    assert_eq!(
        header_of(&res, "content-type"),
        "text/javascript; charset=utf-8"
    );
    assert_eq!(
        header_of(&res, "cache-control"),
        "public, max-age=31536000, immutable"
    );
    assert_eq!(header_of(&res, "vary"), "Accept-Encoding");
    assert_eq!(header_of(&res, "x-content-type-options"), "nosniff");
    let tag = header_of(&res, "etag").to_owned();
    assert_eq!(tag, "\"assets/app-1a2b.js\"");
    let body = http_body_util::BodyExt::collect(res.into_body())
        .await
        .unwrap()
        .to_bytes();
    assert_eq!(
        &body[..2],
        [0x1f, 0x8b],
        "the gzip bytes as they are embedded"
    );
    // A client that does not accept gzip is sent the file itself, under a tag of its own.
    let (res, text) = send(&app, get("/assets/app-1a2b.js")).await;
    assert_eq!(header_of(&res, "content-encoding"), "");
    assert_eq!(header_of(&res, "etag"), "\"assets/app-1a2b.js.identity\"");
    assert_eq!(text, SCRIPT);
    // The browser that holds it is told so.
    let mut again = get("/assets/app-1a2b.js");
    again
        .headers_mut()
        .insert(header::ACCEPT_ENCODING, "gzip".parse().unwrap());
    again
        .headers_mut()
        .insert(header::IF_NONE_MATCH, tag.parse().unwrap());
    let (res, text) = send(&app, again).await;
    assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
    assert!(text.is_empty());
    assert_eq!(
        header_of(&res, "cache-control"),
        "public, max-age=31536000, immutable"
    );
    // A font is embedded as it is.
    let (res, text) = send(&app, get("/assets/font-9z.woff2")).await;
    assert_eq!(
        (header_of(&res, "content-type"), text.as_str()),
        ("font/woff2", "wOF2-not-really")
    );
    // The page names its script by an absolute address, so a deep link finds it.
    let (_, page) = send(&app, get("/live/calls")).await;
    assert!(page.contains("src=\"/assets/app-1a2b.js\""), "{page}");
}

#[tokio::test]
async fn no_address_steps_out_of_the_bundle() {
    let s = state(8, &[]);
    assert!(s.serve_bundle(built()));
    let app = app(&s);
    for path in [
        "/assets/../Cargo.toml",
        "/assets/../../Cargo.toml",
        "/assets/%2e%2e/%2e%2e/Cargo.toml",
        "/assets/..%2f..%2fCargo.toml",
        "/assets/%2e%2e%5c%2e%2e%5cCargo.toml",
        "/assets//etc/passwd",
        "/assets/app-1a2b.js/../../../Cargo.toml",
        "/assets/",
        "/assets",
        "/assets/app-1a2b.js.gz",
        "/assets/manifest.rs",
    ] {
        let (res, text) = send(&app, get(path)).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "{path}: {text}");
        assert!(
            !text.contains("[package]") && !text.contains("root:"),
            "{path}: {text}"
        );
    }
    // Outside /assets/ an address that is no file is a page of the app, never a file of the disk.
    for path in [
        "/../Cargo.toml",
        "/%2e%2e/Cargo.toml",
        "/web-dist/manifest.rs",
        "/source-hash.txt",
        "/etc/passwd",
    ] {
        let (res, text) = send(&app, get(path)).await;
        assert_eq!(res.status(), StatusCode::OK, "{path}");
        assert!(text.starts_with("<html>"), "{path}: {text}");
        assert!(!text.contains("[package]"), "{path}: {text}");
    }
}
