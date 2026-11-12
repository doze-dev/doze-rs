//! What one console connection may cost: how long a request may take to arrive, how many connections and event streams
//! may be open at once, and the answer to one that is over (audit A12).
//!
//! The console has no login and listens where the operator says, so a client that opens connections and says nothing
//! must not be able to hold the broker's descriptors or memory. The numbers are written in `behaviours/console.md`
//! (CON-25 to CON-27) and the connections are counted in the broker's descriptor budget (`doze-kafka`'s `descriptors.rs`).

use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE, RETRY_AFTER};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// The most connections open at once, event streams included. Over it a connection is answered 503 and closed.
pub const MAX_CONNECTIONS: usize = 256;
/// The most event streams (`/events`) open at once. Over it a stream is answered 503.
pub const MAX_STREAMS: usize = 64;
/// How long a connection may take to send a request head, and how long a connection that has nothing to say may stay
/// open (hyper arms the same timer before every request head, so an idle keep-alive connection ends with it).
pub const HEADER_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a request may take to be answered, its body included. A stream is answered at once (its head), and then lives
/// until the client or the server ends it.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// What `Retry-After` says (seconds) on a 503.
pub const RETRY_AFTER_SECS: &str = "1";

/// The limits of one console. [`Limits::default`] is the documented numbers; a test makes them small.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_connections: usize,
    pub max_streams: usize,
    pub header_timeout: Duration,
    pub request_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_connections: MAX_CONNECTIONS,
            max_streams: MAX_STREAMS,
            header_timeout: HEADER_TIMEOUT,
            request_timeout: REQUEST_TIMEOUT,
        }
    }
}

/// 503 with `Retry-After`: a plain-text answer (the security headers are added by their layer).
#[must_use]
pub fn busy(what: &str) -> Response {
    let mut res = (
        StatusCode::SERVICE_UNAVAILABLE,
        [(CONTENT_TYPE, "text/plain; charset=utf-8")],
        format!("{what}\n"),
    )
        .into_response();
    res.headers_mut()
        .insert(RETRY_AFTER, HeaderValue::from_static(RETRY_AFTER_SECS));
    res
}

/// The bytes of the 503 a connection over the cap is answered with before it is closed: it never reaches the router, so
/// the head is written here, with the same security headers every answer carries.
#[must_use]
pub fn refusal_bytes() -> Vec<u8> {
    let body = "The console has as many connections open as it allows. Try again in a moment.\n";
    format!(
        "HTTP/1.1 503 Service Unavailable\r\nRetry-After: {RETRY_AFTER_SECS}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n{}Cache-Control: no-store\r\n\r\n{body}",
        body.len(),
        crate::headers::SECURITY_LINES,
    )
    .into_bytes()
}

/// 408 for a request that was not complete in time (a body that never arrives).
pub async fn request_timeout(State(limit): State<Duration>, req: Request, next: Next) -> Response {
    let sent_a_body = req.headers().contains_key(CONTENT_LENGTH);
    match tokio::time::timeout(limit, next.run(req)).await {
        Ok(res) => res,
        Err(_) => {
            let mut res = (
                StatusCode::REQUEST_TIMEOUT,
                [(CONTENT_TYPE, "text/plain; charset=utf-8")],
                if sent_a_body {
                    "The request body did not arrive in time.\n"
                } else {
                    "The request was not answered in time.\n"
                },
            )
                .into_response();
            res.headers_mut()
                .insert(axum::http::header::CONNECTION, HeaderValue::from_static("close"));
            res
        }
    }
}
