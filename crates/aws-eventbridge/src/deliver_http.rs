//! A target that is an API destination: one HTTP request to the destination's endpoint, with the connection's credentials.
//!
//! What is built: `*` in the endpoint filled from the target's `HttpParameters.PathParameterValues`, the target's header and query
//! string parameters added, the connection's invocation header, query string and body parameters laid over them (the connection wins,
//! as documented), Basic and API key credentials, and a JSON body (the connection's body parameters join it when it is an object).
//! **Not built:** OAuth client credentials (the token request is deferred, SCOPE.md) and `https://` endpoints (no TLS client). Both are
//! stored, and not delivered, and the log says so. A request waits at most five seconds, inside the actor.
//!
//! A 429 or a 5xx, or no answer, may pass and is retried under the target's retry policy; any other status is the endpoint refusing
//! the event and is not.

use serde_json::Value;

use crate::core::Core;
use crate::deliver::{Delivery, Failure, Outcome};
use crate::http::{self, Request, Sent};
use crate::model::{ApiDestination, Connection, key};
use crate::names::Parts;

impl Core {
    pub(crate) async fn to_api_destination(&self, d: &Delivery) -> Outcome {
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
        let body = with_body_parameters(&d.payload, &params);
        let body = if matches!(dest.method.as_str(), "GET" | "HEAD" | "OPTIONS") {
            ""
        } else {
            body.as_str()
        };
        let sent = http::send(&Request {
            method: &dest.method,
            url: &url,
            headers: &headers,
            body,
        })
        .await;
        match sent {
            Sent::Status(s) if (200..300).contains(&s) => Outcome::Sent,
            Sent::Status(s) => Outcome::Failed(Failure {
                code: format!("HTTP{s}"),
                message: format!("{} answered {s}", dest.endpoint),
                retryable: s == 429 || s >= 500,
            }),
            Sent::Failed(why) => Outcome::Failed(Failure {
                code: "ConnectionFailed".into(),
                message: why,
                retryable: true,
            }),
            Sent::Unsupported(why) => Outcome::NotLocal(why),
        }
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
