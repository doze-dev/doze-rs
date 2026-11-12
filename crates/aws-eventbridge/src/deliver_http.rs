//! A target that is an API destination: one HTTP request to the destination's endpoint, with the connection's credentials.
//!
//! What is built: `*` in the endpoint filled from the target's `HttpParameters.PathParameterValues`, the target's header and query
//! string parameters added, the connection's invocation header, query string and body parameters laid over them (the connection wins,
//! as documented), Basic and API key credentials, and a JSON body (the connection's body parameters join it when it is an object).
//! **Not built:** OAuth client credentials (the token request is deferred, SCOPE.md) and `https://` endpoints (no TLS client). Both are
//! stored, and not delivered, and the log says so.
//!
//! **The request is made off the actor**, as the Go edition made it: the actor builds it from the destination and the connection and
//! hands it to a task, so a slow endpoint (a request waits at most five seconds) never holds the region's other calls. The task applies
//! the target's retry policy itself (`retry::next`, the actor's rule) and sends what runs out to the dead-letter queue; each attempt's
//! result is recorded in [`Calls`]. A 429 or a 5xx, or no answer, may pass and is retried; any other status is the endpoint refusing the
//! event and is not.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use aws_peers::Peers;
use aws_webhook::{Request, Sent};
use aws_wire::service::Caller;
use doze_runtime::Clock;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::core::Core;
use crate::deliver::{Delivery, Failure, Outcome};
use crate::model::{ApiDestination, Connection, key};
use crate::names::Parts;
use crate::retry::{MAX_WAITING, Next, dead_letter, next};

/// How many attempts are remembered.
const KEPT: usize = 100;

/// What the API destination calls of a region came to: the last attempts, and how many calls are under way.
#[derive(Debug, Default)]
pub(crate) struct Calls {
    under_way: AtomicUsize,
    attempts: Mutex<VecDeque<Attempt>>,
}

/// One attempt of a call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Attempt {
    pub(crate) target: String,
    /// The attempt's number, from 1.
    pub(crate) try_number: u32,
    /// `200`, `HTTP503`, `ConnectionFailed`...
    pub(crate) result: String,
    /// Whether another attempt follows.
    pub(crate) retrying: bool,
}

impl Calls {
    fn note(&self, a: Attempt) {
        let mut all = self.attempts.lock().unwrap_or_else(PoisonError::into_inner);
        if all.len() >= KEPT {
            all.pop_front();
        }
        all.push_back(a);
    }

    /// The attempts remembered, oldest first.
    #[cfg(test)]
    pub(crate) fn attempts(&self) -> Vec<Attempt> {
        self.attempts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }

    /// How many calls are being made or waiting to be made again.
    pub(crate) fn under_way(&self) -> usize {
        self.under_way.load(Ordering::Acquire)
    }
}

/// A call to make, with all it needs once it has left the actor.
struct Job {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: String,
    endpoint: String,
    delivery: Delivery,
    caller: Caller,
    peers: Option<Arc<Peers>>,
    clock: Arc<dyn Clock>,
    calls: Arc<Calls>,
    stop: CancellationToken,
}

impl Core {
    pub(crate) fn to_api_destination(&self, d: &Delivery, caller: &Caller) -> Outcome {
        let Some((dest, conn)) = self.destination_of(&d.target.arn) else {
            return Failure::fatal(
                "ResourceNotFoundException",
                "the API destination or its connection is gone",
            );
        };
        if conn.state != "AUTHORIZED" {
            return Failure::fatal(
                "InvalidStateException",
                "the API destination's connection is not authorized",
            );
        }
        let params: Value = serde_json::from_str(&conn.auth_parameters).unwrap_or(Value::Null);
        let mut headers = vec![("Content-Type".to_owned(), "application/json".to_owned())];
        match credential(&conn.auth_type, &params) {
            Ok(Some(h)) => headers.push(h),
            Ok(None) => {}
            Err(why) => return Outcome::NotLocal(why),
        }
        headers.extend(pairs(d.spec.pointer("/HttpParameters/HeaderParameters")));
        headers.extend(list(
            params.pointer("/InvocationHttpParameters/HeaderParameters"),
        ));
        let url = url_of(dest, &d.spec, &params);
        if !url.starts_with("http://") {
            return Outcome::NotLocal(format!(
                "{url} is not an http:// URL; doze-aws carries no TLS client"
            ));
        }
        if self.calls.under_way() >= MAX_WAITING {
            return Failure::fatal(
                "ThrottlingException",
                "too many API destination calls are under way",
            );
        }
        let body = if matches!(dest.method.as_str(), "GET" | "HEAD" | "OPTIONS") {
            String::new()
        } else {
            with_body_parameters(&d.payload, &params)
        };
        let job = Job {
            method: dest.method.clone(),
            url,
            headers,
            body,
            endpoint: dest.endpoint.clone(),
            delivery: d.clone(),
            caller: caller.clone(),
            peers: self.peers.clone(),
            clock: Arc::clone(&self.clock),
            calls: Arc::clone(&self.calls),
            stop: self.tasks.stopping(),
        };
        self.calls.under_way.fetch_add(1, Ordering::AcqRel);
        self.tasks.spawn(call(job));
        Outcome::Handed
    }

    /// The destination and connection a target ARN names.
    fn destination_of(&self, arn: &str) -> Option<(&ApiDestination, &Connection)> {
        let parts = Parts::of(arn).filter(|p| p.kind == "api-destination")?;
        let name = parts.tail.split('/').next()?;
        let dest = self.destinations.get(&key(parts.account, &[name]))?;
        let conn_name = Parts::of(&dest.connection_arn)?.tail.split('/').next()?;
        let conn = self.connections.get(&key(parts.account, &[conn_name]))?;
        (conn.arn == dest.connection_arn).then_some((dest, conn))
    }
}

/// What an answer means for the delivery.
fn judge(endpoint: &str, sent: Sent) -> Result<String, Failure> {
    match sent {
        Sent::Status(s) if (200..300).contains(&s) => Ok(s.to_string()),
        Sent::Status(s) => Err(Failure {
            code: format!("HTTP{s}"),
            message: format!("{endpoint} answered {s}"),
            retryable: s == 429 || s >= 500,
        }),
        Sent::Failed(why) => Err(Failure {
            code: "ConnectionFailed".into(),
            message: why,
            retryable: true,
        }),
        Sent::Unsupported(why) => Err(Failure {
            code: "Unsupported".into(),
            message: why,
            retryable: false,
        }),
    }
}

/// Makes the call, and again under the target's retry policy, off the actor.
async fn call(job: Job) {
    let d = &job.delivery;
    let mut try_number = 1;
    loop {
        let sent = aws_webhook::send(&Request {
            method: &job.method,
            url: &job.url,
            headers: &job.headers,
            body: &job.body,
        })
        .await;
        let mut attempt = Attempt {
            target: d.target.id.clone(),
            try_number,
            result: String::new(),
            retrying: false,
        };
        let why = match judge(&job.endpoint, sent) {
            Ok(status) => {
                attempt.result = status;
                job.calls.note(attempt);
                break;
            }
            Err(why) => why,
        };
        attempt.result.clone_from(&why.code);
        match next(d, &why, try_number, job.clock.now_ms(), true) {
            Next::Retry(wait_ms) => {
                attempt.retrying = true;
                job.calls.note(attempt);
                tracing::debug!(rule = %d.rule_arn, target = %d.target.id, "API destination call failed, will retry: {}", why.message);
                let wait = Duration::from_millis(u64::try_from(wait_ms).unwrap_or(1000));
                tokio::select! {
                    () = tokio::time::sleep(wait) => {}
                    () = job.stop.cancelled() => break,
                }
                try_number += 1;
            }
            Next::GiveUp(condition) => {
                job.calls.note(attempt);
                dead_letter(
                    job.peers.as_deref(),
                    d,
                    &why,
                    try_number,
                    condition,
                    &job.caller,
                )
                .await;
                break;
            }
        }
    }
    job.calls.under_way.fetch_sub(1, Ordering::AcqRel);
}

/// The credential header, if the connection has one this client can make.
fn credential(auth_type: &str, params: &Value) -> Result<Option<(String, String)>, String> {
    match auth_type {
        "BASIC" => {
            let get = |m: &str| {
                params
                    .pointer(&format!("/BasicAuthParameters/{m}"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            };
            let pair = format!("{}:{}", get("Username"), get("Password"));
            Ok(Some((
                "Authorization".into(),
                format!("Basic {}", aws_core::base64::encode(pair.as_bytes())),
            )))
        }
        "API_KEY" => {
            let get = |m: &str| {
                params
                    .pointer(&format!("/ApiKeyAuthParameters/{m}"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            };
            Ok(Some((
                get("ApiKeyName").to_owned(),
                get("ApiKeyValue").to_owned(),
            )))
        }
        other => Err(format!(
            "connections with {other} authorization are not called (the token request is deferred)"
        )),
    }
}

/// A map of strings as pairs.
fn pairs(map: Option<&Value>) -> Vec<(String, String)> {
    map.and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
        .collect()
}

/// A list of `{Key, Value}` parameters as pairs.
fn list(items: Option<&Value>) -> Vec<(String, String)> {
    items
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|p| {
            let get = |m: &str| {
                p.get(m)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            (get("Key"), get("Value"))
        })
        .collect()
}

/// The endpoint with its `*` filled, and the query strings added.
fn url_of(dest: &ApiDestination, spec: &Value, params: &Value) -> String {
    let mut values = spec
        .pointer("/HttpParameters/PathParameterValues")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str);
    let mut url = String::new();
    for c in dest.endpoint.chars() {
        if c == '*' {
            url.push_str(&aws_core::uri::encode(
                values.next().unwrap_or_default().as_bytes(),
                false,
            ));
        } else {
            url.push(c);
        }
    }
    let mut query = pairs(spec.pointer("/HttpParameters/QueryStringParameters"));
    query.extend(list(
        params.pointer("/InvocationHttpParameters/QueryStringParameters"),
    ));
    for (i, (k, v)) in query.iter().enumerate() {
        url.push(if i == 0 && !url.contains('?') {
            '?'
        } else {
            '&'
        });
        url.push_str(&aws_core::uri::encode(k.as_bytes(), false));
        url.push('=');
        url.push_str(&aws_core::uri::encode(v.as_bytes(), false));
    }
    url
}

/// The payload, with the connection's body parameters joined to it when it is a JSON object.
fn with_body_parameters(payload: &str, params: &Value) -> String {
    let extra = list(params.pointer("/InvocationHttpParameters/BodyParameters"));
    if extra.is_empty() {
        return payload.to_owned();
    }
    match serde_json::from_str::<Value>(payload) {
        Ok(Value::Object(mut map)) => {
            for (k, v) in extra {
                map.insert(k, Value::String(v));
            }
            Value::Object(map).to_string()
        }
        _ => payload.to_owned(),
    }
}
