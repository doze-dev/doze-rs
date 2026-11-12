//! The React console's built files (`crates/kafka-console/web-dist/`, written by `bun run build` in `web/` and committed, so
//! `cargo build` needs no bun), embedded with `include_bytes!` and served from memory.
//!
//! - A text file (HTML, JS, CSS, SVG) is embedded gzipped (`<file>.gz`, level 9) and sent as it is to a client that accepts
//!   gzip (every browser); another client gets it inflated. A font is embedded as it is (woff2 is already compressed).
//! - A hashed file (`assets/*`) is cached for a year (`immutable`): its name changes when its content does. `index.html` and
//!   the files at the top are `no-cache`.
//! - Every GET under the console's root that is not `/api/`, `/assets/`, `/classic/` or a file of the bundle answers
//!   `index.html` (client routing: a deep link works). `index.html` is rewritten once at start: its two relative references
//!   (`./assets/`, `./favicon.svg`) become absolute under the prefix, and a `<meta name="doze-base">` tells the client its root.
//! - `web-dist/source-hash.txt` is the hash of the `web/` sources the bundle was built from; a test recomputes it and fails
//!   when the bundle is stale.

use std::collections::HashMap;
use std::io::Read;

use axum::body::Body;
use axum::http::header::{
    ACCEPT_ENCODING, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_TYPE, HeaderValue, VARY,
};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::base::Base;

/// One file of the bundle.
#[derive(Debug, Clone, Copy)]
pub struct WebFile {
    /// Its path under the console's root, without the leading slash (`assets/index-Ab12.js`).
    pub path: &'static str,
    pub content_type: &'static str,
    /// Whether `bytes` is gzipped.
    pub gzip: bool,
    pub bytes: &'static [u8],
}

include!("../web-dist/manifest.rs");

/// The bundle, ready to serve under one root.
#[derive(Debug)]
pub struct Bundle {
    by_path: HashMap<&'static str, WebFile>,
    /// `index.html`, rewritten for the root.
    index: Vec<u8>,
}

/// The cache rule of a hashed file.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";
/// The cache rule of `index.html` and the files at the top: ask every time (they are small).
const NO_CACHE: &str = "no-cache";

fn inflate(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() * 4);
    if flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut out)
        .is_err()
    {
        tracing::warn!("an embedded file could not be inflated");
        out.clear();
    }
    out
}

/// Whether the request accepts gzip (`Accept-Encoding: gzip`, `gzip;q=0` excepted).
fn accepts_gzip(headers: &HeaderMap) -> bool {
    headers
        .get_all(ACCEPT_ENCODING)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .any(|part| {
            let mut bits = part.split(';').map(str::trim);
            let name = bits.next().unwrap_or("");
            let zero = bits.any(|b| {
                b.strip_prefix("q=")
                    .and_then(|q| q.parse::<f32>().ok())
                    .is_some_and(|q| q <= 0.0)
            });
            (name.eq_ignore_ascii_case("gzip") || name == "*") && !zero
        })
}

impl Bundle {
    /// The bundle for a console whose root is `web`.
    #[must_use]
    pub fn build(web: &Base) -> Self {
        let by_path: HashMap<_, _> = FILES.iter().map(|f| (f.path, *f)).collect();
        let index = by_path
            .get("index.html")
            .map(|f| {
                if f.gzip {
                    inflate(f.bytes)
                } else {
                    f.bytes.to_vec()
                }
            })
            .unwrap_or_default();
        let index = rewrite_index(&String::from_utf8_lossy(&index), web).into_bytes();
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

    /// The file at `path` (under the root, no leading slash), if the bundle has it. `index.html` is never served by name
    /// here: [`Bundle::index`] serves it rewritten.
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
        let (body, encoded) = if f.gzip && accepts_gzip(headers) {
            (Body::from(f.bytes), true)
        } else if f.gzip {
            (Body::from(inflate(f.bytes)), false)
        } else {
            (Body::from(f.bytes), false)
        };
        let mut res = (StatusCode::OK, body).into_response();
        let h = res.headers_mut();
        h.insert(CONTENT_TYPE, HeaderValue::from_static(f.content_type));
        h.insert(CACHE_CONTROL, HeaderValue::from_static(cache));
        if f.gzip {
            h.insert(VARY, HeaderValue::from_static("Accept-Encoding"));
        }
        if encoded {
            h.insert(CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        }
        Some(res)
    }

    /// The paths of every file, for the tests.
    pub fn paths(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.by_path.keys().copied()
    }

    /// The embedded size in bytes (gzipped where gzipped), for the weight notes.
    #[must_use]
    pub fn embedded_bytes() -> usize {
        FILES.iter().map(|f| f.bytes.len()).sum()
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
        .replace("\"./favicon.svg\"", &format!("\"{}/favicon.svg\"", attr(root)));
    out.replacen(
        "<head>",
        &format!("<head>\n    <meta name=\"doze-base\" content=\"{}\">", attr(root)),
        1,
    )
}

#[cfg(test)]
mod tests;
