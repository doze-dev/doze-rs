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
    pub user_agent: String,
    /// The client's address, without its port.
    pub peer: String,
    /// The console's own request ([`log::OWN_HEADER`]): answered, not logged.
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
            user_agent: String::new(),
            peer: "127.0.0.1".to_owned(),
            own: false,
            body: Body::Read(Vec::new()),
        }
    }
}

/// An answer. Its type is always [`CONTENT_TYPE`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
    /// Ask for basic credentials (`WWW-Authenticate`).
    pub challenge: bool,
    /// How long to wait before answering: a failed sign-in past the free ones.
    pub delay: Duration,
}

impl Response {
    pub(super) fn json<T: Serialize>(status: u16, value: &T) -> Self {
        let mut body = serde_json::to_vec(value).unwrap_or_default();
        body.push(b'\n');
        Self {
            status,
            body,
            challenge: false,
            delay: Duration::ZERO,
        }
    }

    pub(super) fn ok<T: Serialize>(value: &T) -> Self {
        Self::json(200, value)
    }

    pub(super) fn text(text: String) -> Self {
        Self {
            status: 200,
            body: text.into_bytes(),
            challenge: false,
            delay: Duration::ZERO,
        }
    }

    pub(super) fn error(e: &RegistryError) -> Self {
        Self::json(e.status, e)
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
