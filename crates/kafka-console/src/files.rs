//! One embedded file as an HTTP answer (the React console's bundle, `crate::web`): gzip negotiation, an ETag and its 304, a
//! cache rule. Text is embedded as gzip (`pack` in `web/scripts/build.ts`): a client that accepts gzip gets those bytes as they
//! are (`Content-Encoding: gzip`), any other gets the file inflated (flate2, already in the broker), under its own tag (the two
//! representations of one name must not share an ETag). A `Range` is ignored: the whole file is the answer and
//! `Accept-Ranges: none` says so.

use std::borrow::Cow;
use std::io::Read;

use axum::http::header::{
    ACCEPT_ENCODING, ACCEPT_RANGES, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_TYPE, ETAG,
    IF_NONE_MATCH, VARY,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

/// A file ready to serve.
#[derive(Debug)]
pub struct Served {
    pub content_type: &'static str,
    /// What is in the binary: the gzip bytes when `gzip`.
    pub bytes: &'static [u8],
    /// `bytes` are gzip: serve them with `Content-Encoding: gzip`, or inflate them for a client that does not accept it.
    pub gzip: bool,
}

impl Served {
    /// The file itself: the embedded bytes, inflated when they are gzip.
    #[must_use]
    pub fn inflated(&self) -> Option<Cow<'static, [u8]>> {
        if !self.gzip {
            return Some(Cow::Borrowed(self.bytes));
        }
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(self.bytes)
            .read_to_end(&mut out)
            .ok()?;
        Some(Cow::Owned(out))
    }
}

/// Whether the client lists `gzip` in `Accept-Encoding` with a quality above zero (`*` counts).
pub(crate) fn accepts_gzip(headers: &HeaderMap) -> bool {
    headers
        .get(ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(',').any(|item| {
                let mut parts = item.split(';');
                let coding = parts.next().unwrap_or("").trim();
                let refused = parts.any(|p| {
                    let p = p.trim();
                    p.strip_prefix("q=")
                        .is_some_and(|q| q.trim().parse::<f32>() == Ok(0.0))
                });
                (coding.eq_ignore_ascii_case("gzip") || coding == "*") && !refused
            })
        })
}

/// Whether the request's `If-None-Match` names `tag` (or is `*`): the browser already has this.
#[must_use]
pub fn has_tag(headers: &HeaderMap, tag: &str) -> bool {
    headers
        .get(IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(',')
                .map(str::trim)
                .any(|t| t == "*" || t.trim_start_matches("W/") == tag)
        })
}

/// One embedded file as an answer: the gzip bytes as they are to a client that accepts gzip, inflated for any other (under
/// a tag of its own), a 304 for a tag the client already has. `name` makes the ETag; `cache` is the Cache-Control.
#[must_use]
pub fn packed(f: &Served, name: &str, cache: &'static str, headers: &HeaderMap) -> Response {
    let gzip = f.gzip && accepts_gzip(headers);
    let tag = if f.gzip && !gzip {
        format!("\"{name}.identity\"")
    } else {
        format!("\"{name}\"")
    };
    let mut res = if has_tag(headers, &tag) {
        StatusCode::NOT_MODIFIED.into_response()
    } else if f.gzip && !gzip {
        match f.inflated() {
            Some(plain) => ([(CONTENT_TYPE, f.content_type)], plain.into_owned()).into_response(),
            None => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    } else {
        ([(CONTENT_TYPE, f.content_type)], f.bytes).into_response()
    };
    let sent = res.status() == StatusCode::OK;
    let h = res.headers_mut();
    if f.gzip {
        h.insert(VARY, HeaderValue::from_static("Accept-Encoding"));
        if gzip && sent {
            h.insert(CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        }
    }
    h.insert(CACHE_CONTROL, HeaderValue::from_static(cache));
    h.insert(ACCEPT_RANGES, HeaderValue::from_static("none"));
    if let Ok(v) = HeaderValue::from_str(&tag) {
        h.insert(ETAG, v);
    }
    res
}

/// `GET /healthz`.
pub async fn health() -> &'static str {
    "ok\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(name: &str, value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap_or(ACCEPT_ENCODING),
            HeaderValue::from_str(value).unwrap_or(HeaderValue::from_static("")),
        );
        h
    }

    #[test]
    fn gzip_is_accepted_when_listed_with_a_quality_above_zero() {
        for (value, yes) in [
            ("gzip", true),
            ("gzip, deflate, br", true),
            ("br;q=1.0, GZIP;q=0.5", true),
            ("*", true),
            ("gzip;q=0", false),
            ("gzip;q=0.0", false),
            ("deflate", false),
            ("identity", false),
        ] {
            assert_eq!(
                accepts_gzip(&with("accept-encoding", value)),
                yes,
                "{value}"
            );
        }
        assert!(!accepts_gzip(&HeaderMap::new()));
    }

    #[test]
    fn a_tag_the_browser_holds_is_matched_weak_or_strong_or_star() {
        assert!(has_tag(&with("if-none-match", "\"a\""), "\"a\""));
        assert!(has_tag(&with("if-none-match", "W/\"a\", \"b\""), "\"a\""));
        assert!(has_tag(&with("if-none-match", "*"), "\"a\""));
        assert!(!has_tag(&with("if-none-match", "\"b\""), "\"a\""));
        assert!(!has_tag(&HeaderMap::new(), "\"a\""));
    }
}
