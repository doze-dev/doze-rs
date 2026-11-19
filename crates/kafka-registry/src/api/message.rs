//! A request as the server read it, an answer, and how the API is served.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;

use crate::error::RegistryError;

/// A request's body: read, or past its limit (and not kept).
#[derive(Debug, Clone)]
pub enum Body {
    Read(Vec<u8>),
    TooLarge(usize),
}

/// A request, as the server read it.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    /// The path as sent, escapes and all, without the query.
    pub path: String,
    /// The raw query string.
    pub query: String,
    pub host: String,
    pub authorization: Option<String>,
    pub content_type: Option<String>,
    /// The `Accept` header: which of the registry's types the answer is sent as (none acceptable is 406).
    pub accept: Option<String>,
    pub user_agent: String,
    /// The client's address, without its port.
    pub peer: String,
    /// The console's own request (`log::OWN_HEADER`): answered, not logged.
    pub own: bool,
    pub body: Body,
}

impl Request {
    /// A request with no body and nothing else set: a test's starting point.
    #[must_use]
    pub fn new(method: &str, target: &str) -> Self {
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        Self {
            method: method.to_owned(),
            path: path.to_owned(),
            query: query.to_owned(),
            host: "127.0.0.1:8081".to_owned(),
            authorization: None,
            content_type: None,
            accept: None,
            user_agent: String::new(),
            peer: "127.0.0.1".to_owned(),
            own: false,
            body: Body::Read(Vec::new()),
        }
    }
}

/// An answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    /// The body as Confluent writes it: compact JSON with no line end after it.
    pub body: Vec<u8>,
    /// Its `Content-Type`: the registry's type (or the one `Accept` chose), `text/plain` for OPTIONS, the web server's
    /// HTML for a path it cannot read, or none for the misses Confluent's web framework answers itself (404, 405, 406).
    pub content_type: Option<&'static str>,
    /// The `Allow` header of an OPTIONS answer.
    pub allow: Option<String>,
    /// Ask for basic credentials (`WWW-Authenticate`).
    pub challenge: bool,
    /// How long to wait before answering: a failed sign-in past the free ones.
    pub delay: Duration,
}

impl Response {
    pub(super) fn json<T: Serialize>(status: u16, value: &T) -> Self {
        Self::raw(status, serde_json::to_vec(value).unwrap_or_default())
    }

    pub(super) fn raw(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            body,
            content_type: Some(super::CONTENT_TYPE),
            allow: None,
            challenge: false,
            delay: Duration::ZERO,
        }
    }

    pub(super) fn ok<T: Serialize>(value: &T) -> Self {
        Self::json(200, value)
    }

    pub(super) fn text(text: String) -> Self {
        Self::raw(200, text.into_bytes())
    }

    pub(super) fn error(e: &RegistryError) -> Self {
        Self::json(e.status, e)
    }

    /// A miss Confluent's web framework (Jersey) answers before the registry sees the request: the registry's JSON error
    /// shape with no `Content-Type`.
    pub(super) fn framework(status: u16, reason: &str) -> Self {
        let mut r = Self::error(&RegistryError::http(
            status,
            format!("HTTP {status} {reason}"),
        ));
        r.content_type = None;
        r
    }

    /// The answer when answering failed (a handler panicked).
    #[must_use]
    pub fn internal() -> Self {
        Self::raw(
            500,
            br#"{"error_code":50001,"message":"Internal error"}"#.to_vec(),
        )
    }

    /// What Confluent's web server (Jetty) answers to a path it cannot read: its own HTML page.
    pub(super) fn bad_message() -> Self {
        let mut r = Self::raw(
            400,
            b"<h1>Bad Message 400</h1><pre>reason: Bad Request</pre>".to_vec(),
        );
        r.content_type = Some("text/html;charset=iso-8859-1");
        r
    }
}

/// Checks a user and password: the broker's SASL users.
pub type Auth = Arc<dyn Fn(&str, &str) -> bool + Send + Sync>;

/// How the API is served.
#[derive(Clone, Default)]
pub struct Options {
    /// When set, every request must carry basic credentials it approves.
    pub auth: Option<Auth>,
    /// The host names beyond the always-allowed ones (IP literals, `localhost`) a request's `Host` may name; `*` allows
    /// any. The defence against DNS rebinding.
    pub allowed_hosts: Vec<String>,
}

impl std::fmt::Debug for Options {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Options")
            .field("auth", &self.auth.is_some())
            .field("allowed_hosts", &self.allowed_hosts)
            .finish()
    }
}
