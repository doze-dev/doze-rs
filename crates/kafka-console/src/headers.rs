//! Security headers on every answer, the console's own refusals included.

use axum::extract::Request;
use axum::http::HeaderValue;
use axum::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS,
    X_FRAME_OPTIONS,
};
use axum::middleware::Next;
use axum::response::Response;

/// The policy, as a literal that `concat!` can join.
macro_rules! csp {
    () => {
        "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"
    };
}

/// What a page may load and do. Scripts are the console's own files (no inline script, no eval); styles may be inline
/// because positions on the ruler and the lag bars are custom properties (`style="--p:68%"`); nothing may frame it, and
/// forms may only post to it.
pub const CSP: &str = csp!();

/// The four headers that do not depend on the answer, as the lines of a hand-written HTTP head (for the 503 that a
/// connection over the cap is answered with, which never reaches the router).
pub const SECURITY_LINES: &str = concat!(
    "Content-Security-Policy: ",
    csp!(),
    "\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: same-origin\r\nX-Frame-Options: DENY\r\n"
);

/// Adds the headers; a handler that set its own cache rule keeps it, any other answer is not stored.
pub async fn security_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
    h.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    h.insert(X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    if !h.contains_key(CACHE_CONTROL) {
        h.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    res
}
