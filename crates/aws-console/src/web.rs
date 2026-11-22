//! The console's built files (`crates/aws-console/web-dist/`, written by the bundle's build script and committed, so `cargo build`
//! needs no bun), embedded with `include_bytes!` and served from memory.
//!
//! - A text file (HTML, JS, CSS, SVG) is embedded as its `.gz`, held as a [`files::Served`](crate::files::Served) and answered
//!   by [`crate::files::packed`] (as it is to a client that accepts gzip, inflated for another; ETag and 304). A font is embedded
//!   as it is (woff2 is already compressed).
//! - A hashed file (`assets/*`) is cached for a year (`immutable`): its name changes when its content does. `index.html` and the
//!   files at the top are `no-cache`.
//! - Every GET that is not `/api/`, `/assets/` or a file of the bundle answers `index.html` (client routing: a deep link works).
//!   `index.html` is rewritten once: its relative references (`./assets/`, `./favicon.svg`) become absolute, so a deep link
//!   finds them, and a `<meta name="doze-base">` tells the client its root (always the origin's root here: the console has a
//!   listener of its own and no prefix).
//! - `web-dist/source-hash.txt` is the hash of the sources the bundle was built from (`web/tests.rs` says which files); a test
//!   recomputes it and fails when the bundle is stale. Until the console's pages are built the directory holds a placeholder
//!   (`index.html` alone, and `placeholder` for the hash), which the same test checks instead.
//!
//! The table of files is made when the first page is asked for ([`crate::state::State::bundle`]), not at start.

use std::collections::HashMap;

use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, HeaderValue};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::files::Served;

include!("../web-dist/manifest.rs");

/// The bundle, ready to serve.
#[derive(Debug)]
pub struct Bundle {
    by_path: HashMap<&'static str, &'static Served>,
    /// `index.html`, rewritten.
    index: Vec<u8>,
}

/// The cache rule of a hashed file.
pub const IMMUTABLE: &str = "public, max-age=31536000, immutable";
/// The cache rule of `index.html` and the files at the top: ask every time (they are small).
pub const NO_CACHE: &str = "no-cache";

impl Bundle {
    /// The bundle of this build.
    #[must_use]
    pub fn build() -> Self {
        Self::of(FILES)
    }

    /// A bundle of `files` (the tests serve one with hashed assets, which the placeholder has none of).
    #[must_use]
    pub fn of(files: &'static [(&'static str, Served)]) -> Self {
        let by_path: HashMap<_, _> = files.iter().map(|(p, f)| (*p, f)).collect();
        let index = by_path
            .get("index.html")
            .and_then(|f| f.inflated())
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        let index = rewrite_index(&index).into_bytes();
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

    /// The file at `path` (no leading slash), if the bundle has it, served by [`crate::files::packed`] (gzip negotiation, ETag,
    /// 304). `index.html` is always [`Bundle::index`] (rewritten). The path is looked up as it is written: nothing is read from
    /// a disk, so there is no directory to step out of.
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
        Some(crate::files::packed(f, path, cache, headers))
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

/// `index.html` as served: the relative references made absolute, and the root named for the client.
#[must_use]
pub fn rewrite_index(html: &str) -> String {
    html.replace("\"./assets/", "\"/assets/")
        .replace("\"./favicon.svg\"", "\"/favicon.svg\"")
        .replacen(
            "<head>",
            "<head>\n    <meta name=\"doze-base\" content=\"\">",
            1,
        )
}

#[cfg(test)]
mod tests;
