//! The React console's built files (`crates/kafka-console/web-dist/`, written by `bun run build` in `web/` and committed, so
//! `cargo build` needs no bun), embedded with `include_bytes!` and served from memory.
//!
//! - The same mechanism as the classic console's files (`assets.rs`): a text file (HTML, JS, CSS, SVG) is embedded as its `.gz`
//!   (`pack` of `tools/assets-compress.ts`, called by `web/scripts/build.ts`), held as an [`assets::Served`] and answered by
//!   `handlers::plain::packed` (as it is to a client that accepts gzip, inflated for another; ETag and 304). A font is embedded
//!   as it is (woff2 is already compressed).
//! - A hashed file (`assets/*`) is cached for a year (`immutable`): its name changes when its content does. `index.html` and
//!   the files at the top are `no-cache`.
//! - Every GET under the console's root that is not `/api/`, `/assets/`, `/classic/` or a file of the bundle answers
//!   `index.html` (client routing: a deep link works). `index.html` is rewritten once at start: its two relative references
//!   (`./assets/`, `./favicon.svg`) become absolute under the prefix, and a `<meta name="doze-base">` tells the client its root.
//! - `web-dist/source-hash.txt` is the hash of the `web/` sources the bundle was built from; a test recomputes it and fails
//!   when the bundle is stale.

use std::collections::HashMap;

use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, HeaderValue};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::assets::Served;
use crate::base::Base;

include!("../web-dist/manifest.rs");

/// The bundle, ready to serve under one root.
#[derive(Debug)]
pub struct Bundle {
    by_path: HashMap<&'static str, &'static Served>,
    /// `index.html`, rewritten for the root.
    index: Vec<u8>,
}

/// The cache rule of a hashed file.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";
/// The cache rule of `index.html` and the files at the top: ask every time (they are small).
const NO_CACHE: &str = "no-cache";

impl Bundle {
    /// The bundle for a console whose root is `web`.
    #[must_use]
    pub fn build(web: &Base) -> Self {
        let by_path: HashMap<_, _> = FILES.iter().map(|(p, f)| (*p, f)).collect();
        let index = by_path
            .get("index.html")
            .and_then(|f| f.inflated())
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        let index = rewrite_index(&index, web).into_bytes();
        Self { by_path, index }
    }

    /// `index.html` as served.
    #[must_use]
    pub fn index_html(&self) -> &[u8] {
        &self.index
    }

    /// The answer for `index.html`.
    #[must_use]
    pub fn index(&self) -> Response {
        let mut res = (StatusCode::OK, self.index.clone()).into_response();
        let h = res.headers_mut();
        h.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        );
        h.insert(CACHE_CONTROL, HeaderValue::from_static(NO_CACHE));
        res
    }

    /// The file at `path` (under the root, no leading slash), if the bundle has it, served as the classic console's files
    /// are (`handlers::plain::packed`: gzip negotiation, ETag, 304). `index.html` is always [`Bundle::index`] (rewritten).
    #[must_use]
    pub fn file(&self, path: &str, headers: &HeaderMap) -> Option<Response> {
        if path == "index.html" {
            return Some(self.index());
        }
        let f = self.by_path.get(path)?;
        let cache = if path.starts_with("assets/") {
            IMMUTABLE
        } else {
            NO_CACHE
        };
        Some(crate::handlers::plain::packed(f, path, cache, headers))
    }

    /// The paths of every file, for the tests.
    pub fn paths(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.by_path.keys().copied()
    }

    /// The embedded size in bytes (gzipped where gzipped), for the weight notes.
    #[must_use]
    pub fn embedded_bytes() -> usize {
        FILES.iter().map(|(_, f)| f.bytes.len()).sum()
    }
}

/// `index.html` for the root `web`: the relative references made absolute, and the root named for the client.
#[must_use]
pub fn rewrite_index(html: &str, web: &Base) -> String {
    let root = web.prefix();
    let attr = |s: &str| {
        s.replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('<', "&lt;")
    };
    let out = html
        .replace("\"./assets/", &format!("\"{}/assets/", attr(root)))
        .replace(
            "\"./favicon.svg\"",
            &format!("\"{}/favicon.svg\"", attr(root)),
        );
    out.replacen(
        "<head>",
        &format!(
            "<head>\n    <meta name=\"doze-base\" content=\"{}\">",
            attr(root)
        ),
        1,
    )
}

#[cfg(test)]
mod tests;
