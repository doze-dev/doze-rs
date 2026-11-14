//! WebSocket APIs (decision D8): a client connects at the API's stage address with `Upgrade: websocket`; the `$connect` route (when
//! the API has one) runs first and may refuse; each message is routed by the API's `routeSelectionExpression` (`$request.body.action`)
//! to its route or `$default`; `$disconnect` runs when the connection ends. A backend talks back through the `@connections`
//! management API (`POST`, `GET`, `DELETE /<stage>/@connections/<id>`, the `apigatewaymanagementapi` client pointed at
//! `<endpoint>/_aws/execute-api/<api>/<stage>`), served from its model like any REST service (`management.rs`).

mod frame;
mod session;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use aws_wire::ApiError;
use aws_wire::service::{Upgrade, WebRequest};
use bytes::Bytes;
use serde_json::json;
use tokio::sync::mpsc;

use super::{Find, Kind, reply};
use crate::Inner;

use frame::accept_key;

/// Reads every message out of `raw` as a session would (for the fuzz target): how many were read before the end or an error.
pub(crate) fn read_all(raw: &[u8]) -> usize {
    let mut reader = frame::Reader::default();
    if reader.push(raw).is_err() {
        return 0;
    }
    let mut n = 0;
    while let Ok(Some(_)) = reader.next() {
        n += 1;
    }
    n
}

/// How many messages may wait for a slow client before a post to it is refused.
const QUEUE: usize = 256;

/// What is sent to a connection.
#[derive(Debug)]
pub(crate) enum Out {
    Text(String),
    Close,
}

/// An open connection.
#[derive(Debug)]
struct Conn {
    api: String,
    tx: mpsc::Sender<Out>,
    connected_ms: i64,
    last_ms: i64,
    source_ip: String,
    user_agent: String,
}

/// The open connections. A lock on a small leaf map: the management API and the sessions only insert, look up and remove.
#[derive(Debug, Default)]
pub(crate) struct Connections(Mutex<HashMap<String, Conn>>);

impl Connections {
    fn with<T>(&self, f: impl FnOnce(&mut HashMap<String, Conn>) -> T) -> T {
        f(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }

    pub(crate) fn touch(&self, id: &str, now: i64) {
        self.with(|m| {
            if let Some(c) = m.get_mut(id) {
                c.last_ms = now;
            }
        });
    }

    pub(crate) fn remove(&self, id: &str) {
        self.with(|m| m.remove(id));
    }
}

/// What the management API asks of a connection.
pub(crate) enum Manage {
    Post(Bytes),
    Get,
    Delete,
}

/// A connection that is not there (any more). (memory) AWS's message.
fn gone(id: &str) -> ApiError {
    ApiError::new(410, "GoneException", format!("Connection {id} is gone"))
}

/// The `@connections` API (`management.rs`): post to a connection of `api`, read it, or close it. The answer is the operation's
/// output, by the model's wire names.
pub(crate) fn manage(
    inner: &Inner,
    api: &str,
    id: &str,
    what: Manage,
) -> Result<serde_json::Value, ApiError> {
    let found = inner.plane.ws.with(|m| {
        m.get(id).filter(|c| c.api == api).map(|c| {
            (
                c.tx.clone(),
                c.connected_ms,
                c.last_ms,
                c.source_ip.clone(),
                c.user_agent.clone(),
            )
        })
    });
    let Some((tx, connected, last, ip, agent)) = found else {
        return Err(gone(id));
    };
    let iso = |ms: i64| aws_core::time::format_iso8601(ms / 1000);
    match what {
        Manage::Post(data) => {
            match tx.try_send(Out::Text(String::from_utf8_lossy(&data).into_owned())) {
                Ok(()) => Ok(json!({})),
                // (memory)
                Err(mpsc::error::TrySendError::Full(_)) => Err(ApiError::new(
                    429,
                    "LimitExceededException",
                    "Rate exceeded",
                )),
                Err(mpsc::error::TrySendError::Closed(_)) => Err(gone(id)),
            }
        }
        Manage::Get => Ok(json!({
            "connectedAt": iso(connected),
            "identity": {"sourceIp": ip, "userAgent": agent},
            "lastActiveAt": iso(last),
        })),
        Manage::Delete => {
            drop(tx.try_send(Out::Close));
            Ok(json!({}))
        }
    }
}

/// A connection id: 12 random bytes in base64, as AWS's look.
fn connection_id() -> String {
    let mut b = [0u8; 12];
    // WHY: with no randomness the id is still unique enough for one process: the request id is mixed in below.
    if getrandom::fill(&mut b).is_err() {
        b.copy_from_slice(
            aws_wire::RequestId::new()
                .as_str()
                .as_bytes()
                .get(..12)
                .unwrap_or(&[0; 12]),
        );
    }
    aws_core::base64::encode(&b)
}

/// Records an open connection; the receiver its session reads posts from.
fn register(inner: &Inner, id: &str, api: &str, req: &super::Incoming) -> mpsc::Receiver<Out> {
    let (tx, rx) = mpsc::channel(QUEUE);
    let conn = Conn {
        api: api.to_owned(),
        tx,
        connected_ms: req.now_ms,
        last_ms: req.now_ms,
        source_ip: "127.0.0.1".into(),
        user_agent: req.header("user-agent").unwrap_or_default().to_owned(),
    };
    inner.plane.ws.with(|m| m.insert(id.to_owned(), conn));
    rx
}

/// A request to open a WebSocket at an API's stage.
pub(crate) async fn upgrade(inner: Arc<Inner>, request: WebRequest) -> Upgrade {
    let refuse = |status: u16, msg: &str| Upgrade::Refused(reply::message(status, msg));
    let Some(addr) = super::addr(&request.host, &request.path) else {
        return refuse(404, "Not Found");
    };
    let Some(key) = request
        .headers
        .iter()
        .find(|(k, _)| k == "sec-websocket-key")
        .map(|(_, v)| v.clone())
    else {
        return refuse(400, "Bad Request");
    };
    let find = Find {
        api: addr.api.clone(),
        rest: addr.rest.clone(),
        method: "GET".into(),
        stage_in_path: true,
        route_key: Some("$connect".into()),
    };
    let found = match super::locate(&inner, addr.region.as_deref(), find).await {
        Ok(f) if f.kind == Kind::WebSocket => f,
        Ok(_) => return refuse(400, "Bad Request"),
        Err(_) => return refuse(403, "Forbidden"),
    };
    let incoming = super::Incoming {
        method: "GET".into(),
        query: super::mapping::query_pairs(&request.query),
        raw_query: request.query,
        headers: request.headers,
        body: Bytes::new(),
        host: request.host,
        request_id: aws_wire::RequestId::new().as_str().to_owned(),
        now_ms: inner.clock.now_ms(),
    };
    let id = connection_id();
    if let Err(status) = session::connect(&inner, &found, &incoming, &id).await {
        return refuse(
            status,
            if status == 401 {
                "Unauthorized"
            } else {
                "Forbidden"
            },
        );
    }
    let rx = register(&inner, &id, &addr.api, &incoming);
    let headers = vec![
        ("Upgrade".to_owned(), "websocket".to_owned()),
        ("Connection".to_owned(), "Upgrade".to_owned()),
        ("Sec-WebSocket-Accept".to_owned(), accept_key(&key)),
    ];
    let session: aws_wire::service::Session = Box::new(move |socket| {
        Box::pin(session::run(inner, found, id, rx, socket, incoming.now_ms))
    });
    Upgrade::Accepted { headers, session }
}
