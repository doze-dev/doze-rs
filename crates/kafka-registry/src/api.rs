//! The registry's REST interface, without the network: a [`Request`] in, a [`Response`] out, so the tests answer requests
//! as the server does with no socket. The checks run in Confluent's order and the oracle's: the `Host` header first
//! (before the credentials and the route: a request through a name somebody else controls learns nothing), then the
//! credentials, then the body's type, then the route, then the body's size.

mod confluent;
mod extension;
pub mod log;
pub mod route;
pub mod text;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::RegistryError;
use crate::registry::{Origin, Registry};
use log::{LoggedRequest, MAX_REQUEST_URL, RequestLog};
use route::{Captures, Miss, Route};

/// What Confluent's registry answers with, and what its clients send.
pub const CONTENT_TYPE: &str = "application/vnd.schemaregistry.v1+json";

/// A schema with its references may be as large as Confluent's registry takes (recorded: large-bodies, a 9 MiB schema).
pub const MAX_SCHEMA_BODY: usize = 20 << 20;
/// A level or a mode is a few bytes: a larger body is refused unread (Confluent reads 20 MiB of padding and ignores it).
pub const MAX_SETTING_BODY: usize = 64 << 10;

/// The longest User-Agent a version keeps.
const MAX_AGENT_LEN: usize = 256;

/// Failed sign-ins from one address are answered at once up to this many in a row; after that each waits
/// [`FAILURE_STEP`] longer than the last, up to [`MAX_FAILURE_WAIT`]. A success clears the count.
const FREE_FAILURES: u32 = 5;
const FAILURE_STEP: Duration = Duration::from_millis(100);
const MAX_FAILURE_WAIT: Duration = Duration::from_secs(2);
/// Addresses remembered; past this the counts start again.
const MAX_FAIL_ADDRS: usize = 4096;

/// The most a request to `path` may bring.
#[must_use]
pub fn body_limit(path: &str) -> usize {
    if path.starts_with("/config") || path.starts_with("/mode") {
        MAX_SETTING_BODY
    } else {
        MAX_SCHEMA_BODY
    }
}

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
    fn json<T: Serialize>(status: u16, value: &T) -> Self {
        let mut body = serde_json::to_vec(value).unwrap_or_default();
        body.push(b'\n');
        Self {
            status,
            body,
            challenge: false,
            delay: Duration::ZERO,
        }
    }

    fn ok<T: Serialize>(value: &T) -> Self {
        Self::json(200, value)
    }

    fn text(text: String) -> Self {
        Self {
            status: 200,
            body: text.into_bytes(),
            challenge: false,
            delay: Duration::ZERO,
        }
    }

    fn error(e: &RegistryError) -> Self {
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

/// The REST interface over a registry.
#[derive(Debug)]
pub struct Api {
    registry: Arc<Registry>,
    options: Options,
    fails: Mutex<HashMap<String, u32>>,
    log: Mutex<RequestLog>,
}

/// What a handler is given.
pub(crate) struct Call<'a> {
    pub(crate) req: &'a Request,
    pub(crate) at: Captures,
    pub(crate) user: String,
}

impl Call<'_> {
    pub(crate) fn subject(&self) -> &str {
        self.at.subject.as_deref().unwrap_or_default()
    }

    pub(crate) fn version(&self) -> &str {
        self.at.version.as_deref().unwrap_or_default()
    }

    pub(crate) fn flag(&self, name: &str) -> bool {
        text::flag(&self.req.query, name)
    }

    pub(crate) fn query(&self, name: &str) -> Option<String> {
        text::query(&self.req.query, name)
    }

    pub(crate) fn origin(&self) -> Origin {
        Origin {
            user: self.user.clone(),
            from: self.req.peer.clone(),
            agent: text::cut(&self.req.user_agent, MAX_AGENT_LEN).to_owned(),
        }
    }

    /// The body, for a handler that reads it.
    pub(crate) fn body(&self) -> Result<&[u8], RegistryError> {
        match &self.req.body {
            Body::Read(b) => Ok(b),
            Body::TooLarge(limit) => Err(RegistryError::http(
                413,
                format!("The request body is larger than {limit} bytes"),
            )),
        }
    }
}

/// A POST or PUT body of this type is read, as Confluent decides (recorded: content-types): none, JSON, the registry's
/// own types and `application/octet-stream`. The three a web page may send to another site without asking first
/// (`text/plain`, forms, multipart) are refused, so a page cannot register schemas in a developer's registry.
fn accepted_body_type(ct: Option<&str>) -> bool {
    let Some(ct) = ct.filter(|c| !c.trim().is_empty()) else {
        return true;
    };
    let media = ct
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    matches!(
        media.as_str(),
        "application/json"
            | "application/octet-stream"
            | "application/vnd.schemaregistry+json"
            | "application/vnd.schemaregistry.v1+json"
    )
}

impl Api {
    #[must_use]
    pub fn new(registry: Arc<Registry>, options: Options) -> Self {
        Self {
            registry,
            options,
            fails: Mutex::default(),
            log: Mutex::default(),
        }
    }

    /// The registry behind the API.
    #[must_use]
    pub fn registry(&self) -> &Arc<Registry> {
        &self.registry
    }

    /// Answers one request, logs it and keeps it in the request log.
    // fuzz:parser registry_api
    #[must_use]
    pub fn serve(&self, req: &Request) -> Response {
        let start = Instant::now();
        let (resp, at, user) = self.answer(req);
        let took = start.elapsed();
        log_request(req, resp.status, &user, took);
        self.record(req, &resp, &at, user, took);
        resp
    }

    fn answer(&self, req: &Request) -> (Response, Captures, String) {
        let none = Captures::default();
        if !kafka_auth::hostguard::allowed(&req.host, &self.options.allowed_hosts) {
            let msg = format!(
                "This schema registry does not answer to the host name {}. If that is a name you chose, start doze-kafka with --console-allow-host={} (the registry answers to the same names as the console, and to --advertised-host).",
                text::quoted(&req.host),
                text::cut(text::host_name(&req.host), 200)
            );
            return (
                Response::error(&RegistryError::http(403, msg)),
                none,
                String::new(),
            );
        }
        let user = match self.sign_in(req) {
            Ok(user) => user,
            Err(resp) => return (resp, none, String::new()),
        };
        if matches!(req.method.as_str(), "POST" | "PUT")
            && !accepted_body_type(req.content_type.as_deref())
        {
            let e = RegistryError::http(415, "HTTP 415 Unsupported Media Type");
            return (Response::error(&e), none, user);
        }
        let Some(segs) = text::segments(&req.path) else {
            return (
                Response::error(&RegistryError::http(400, "HTTP 400 Bad Request")),
                none,
                user,
            );
        };
        let (route, at) = match route::find(&req.method, &segs) {
            Ok(found) => found,
            Err(Miss::NotFound) => {
                return (
                    Response::error(&RegistryError::http(404, "HTTP 404 Not Found")),
                    none,
                    user,
                );
            }
            Err(Miss::MethodNotAllowed) => {
                let e = RegistryError::http(405, "HTTP 405 Method Not Allowed");
                return (Response::error(&e), none, user);
            }
        };
        let call = Call { req, at, user };
        let resp = match self.dispatch(route, &call) {
            Ok(r) => r,
            Err(e) => Response::error(&e),
        };
        (resp, call.at, call.user)
    }

    fn dispatch(&self, route: Route, call: &Call<'_>) -> Result<Response, RegistryError> {
        match route {
            Route::DozeOverview
            | Route::DozeRequests
            | Route::DozeVersion
            | Route::DozeResolved
            | Route::DozeCheck => self.extension(route, call),
            _ => confluent::answer(&self.registry, route, call),
        }
    }

    /// The user a request signed in as (`""` with no authentication), or the 401 that refuses it.
    fn sign_in(&self, req: &Request) -> Result<String, Response> {
        let creds = req.authorization.as_deref().and_then(text::basic);
        let Some(auth) = &self.options.auth else {
            return Ok(creds.map(|(u, _)| u).unwrap_or_default());
        };
        let mut fails = self.fails.lock().unwrap_or_else(PoisonError::into_inner);
        match creds {
            Some((user, pass)) if auth(&user, &pass) => {
                fails.remove(&req.peer);
                Ok(user)
            }
            _ => {
                if fails.len() >= MAX_FAIL_ADDRS {
                    fails.clear();
                }
                let n = fails.entry(req.peer.clone()).or_default();
                *n = n.saturating_add(1);
                let mut resp = Response::error(&RegistryError::http(401, "Unauthorized"));
                resp.challenge = true;
                resp.delay = n.checked_sub(FREE_FAILURES).map_or(Duration::ZERO, |over| {
                    (FAILURE_STEP * over).min(MAX_FAILURE_WAIT)
                });
                Err(resp)
            }
        }
    }

    /// Keeps a request in the log, unless it is the console's own or an extension route.
    fn record(&self, req: &Request, resp: &Response, at: &Captures, user: String, took: Duration) {
        if req.own || req.path.starts_with("/_doze/") {
            return;
        }
        // The query is kept with the path: it changes what was asked (?verbose=true, ?permanent=true) and carries nothing
        // secret.
        let full = if req.query.is_empty() {
            req.path.clone()
        } else {
            format!("{}?{}", req.path, req.query)
        };
        let mut r = LoggedRequest {
            seq: 0,
            at: crate::registry::rfc3339_now(),
            method: req.method.clone(),
            path: text::cut(&full, MAX_REQUEST_URL).to_owned(),
            status: resp.status,
            took_nanos: 0,
            user,
            from: req.peer.clone(),
            agent: text::cut(&req.user_agent, MAX_AGENT_LEN).to_owned(),
            subject: text::cut(at.subject.as_deref().unwrap_or_default(), MAX_REQUEST_URL)
                .to_owned(),
            id: at.id.as_deref().and_then(text::atoi),
        };
        r.took(took);
        self.log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .add(r);
    }

    /// How many requests the log holds.
    #[must_use]
    pub fn logged(&self) -> usize {
        self.log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

/// One record per request: a server failure as a warning, a refused sign-in, host or incompatible schema at info,
/// anything else at debug. The user is the one the request named; a password is never logged.
fn log_request(req: &Request, status: u16, user: &str, took: Duration) {
    let (method, path, from) = (req.method.as_str(), req.path.as_str(), req.peer.as_str());
    match status {
        500.. => tracing::warn!(
            method,
            path,
            status,
            user,
            from,
            ?took,
            "schema registry request"
        ),
        401 | 403 | 409 => tracing::info!(
            method,
            path,
            status,
            user,
            from,
            ?took,
            "schema registry request"
        ),
        _ => tracing::debug!(
            method,
            path,
            status,
            user,
            from,
            ?took,
            "schema registry request"
        ),
    }
}

#[cfg(test)]
mod tests;
