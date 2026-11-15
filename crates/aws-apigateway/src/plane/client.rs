//! HTTP integrations: one HTTP/1.1 request through `aws-webhook` (plain TCP for `http://`, TLS for `https://`), the answer read whole
//! up to 10 MB. A backend's certificate must be one the machine trusts, unless the integration's `tlsConfig.insecureSkipVerification`
//! is set (REST APIs): then a certificate no trusted authority signed is accepted if it is for the name called, as AWS accepts a
//! self-signed or private-CA certificate there. A refused certificate is logged with the flag that fixes it.

use std::time::Duration;

use aws_webhook::{Failure, Request};
use aws_wire::service::WebReply;
use bytes::Bytes;
use serde_json::Value;

use super::custom::{Backend, Mapped};
use super::mapping::rest_source;
use super::{Found, Incoming, Route, reply};
use crate::doc::text;

const MAX_ANSWER: usize = 10 * 1024 * 1024;

/// A request to send.
#[derive(Debug)]
pub(crate) struct Out {
    pub(crate) method: String,
    pub(crate) url: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Bytes,
    pub(crate) timeout: Duration,
    /// `tlsConfig.insecureSkipVerification`.
    pub(crate) skip_verification: bool,
    /// The most bytes of the answer read.
    pub(crate) limit: usize,
}

impl Out {
    /// A request with API Gateway's defaults: verified TLS, a 10 MB answer.
    pub(crate) fn new(method: String, url: String, timeout: Duration) -> Self {
        Self {
            method,
            url,
            headers: Vec::new(),
            body: Bytes::new(),
            timeout,
            skip_verification: false,
            limit: MAX_ANSWER,
        }
    }
}

/// What came back.
#[derive(Debug)]
pub(crate) struct In {
    pub(crate) status: u16,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Bytes,
}

/// Sends a request.
pub(crate) async fn call(out: &Out) -> Result<In, Failure> {
    let r = Request {
        headers: &out.headers,
        body: &out.body,
        timeout: out.timeout,
        limit: out.limit,
        skip_verification: out.skip_verification,
        ..Request::new(&out.method, &out.url)
    };
    aws_webhook::fetch(&r).await.map(|a| In {
        status: a.status,
        headers: a.headers,
        body: Bytes::from(a.body),
    })
}

/// `tlsConfig.insecureSkipVerification` of an integration.
pub(crate) fn skips_verification(integration: &Value) -> bool {
    integration
        .pointer("/tlsConfig/insecureSkipVerification")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// The integration URI with `{name}` filled from the request's path variables and `integration.request.path` mappings, then the
/// mapped querystring added.
fn url_of(found: &Found, route: &Route, req: &Incoming, ctx: &Value, keep_query: bool) -> String {
    let vars = found
        .variables()
        .map_or(Value::Null, |m| Value::Object(m.clone()));
    let mut url = super::lambda::fill_vars(
        text(&route.integration, "uri").unwrap_or_default(),
        found.variables(),
    );
    let params = crate::doc::obj(&route.integration, "requestParameters");
    for (name, value) in &route.params {
        url = url
            .replace(&format!("{{{name}}}"), value)
            .replace(&format!("{{{name}+}}"), value);
    }
    let mut query: Vec<(String, String)> = if keep_query {
        req.query.clone()
    } else {
        Vec::new()
    };
    for (k, src) in params.into_iter().flatten() {
        let Some(v) = src
            .as_str()
            .and_then(|s| rest_source(s, req, &route.params, ctx, &vars))
        else {
            continue;
        };
        if let Some(p) = k.strip_prefix("integration.request.path.") {
            url = url
                .replace(&format!("{{{p}}}"), &v)
                .replace(&format!("{{{p}+}}"), &v);
        } else if let Some(q) = k.strip_prefix("integration.request.querystring.") {
            query.retain(|(n, _)| n != q);
            query.push((q.to_owned(), v));
        }
    }
    super::mapping::with_query(&mut url, &query);
    url
}

fn mapped_headers(
    route: &Route,
    req: &Incoming,
    ctx: &Value,
    found: &Found,
) -> Vec<(String, String)> {
    let vars = found
        .variables()
        .map_or(Value::Null, |m| Value::Object(m.clone()));
    crate::doc::obj(&route.integration, "requestParameters")
        .into_iter()
        .flatten()
        .filter_map(|(k, src)| {
            let name = k.strip_prefix("integration.request.header.")?;
            Some((
                name.to_owned(),
                rest_source(src.as_str()?, req, &route.params, ctx, &vars)?,
            ))
        })
        .collect()
}

fn timeout(route: &Route) -> Duration {
    Duration::from_millis(
        route
            .integration
            .get("timeoutInMillis")
            .and_then(Value::as_u64)
            .unwrap_or(29_000),
    )
}

/// An HTTP_PROXY integration: the request passed on, the answer passed back.
pub(crate) async fn proxy(found: &Found, route: &Route, req: &Incoming) -> WebReply {
    let method = match text(&route.integration, "httpMethod") {
        Some(m) if m != "ANY" => m.to_owned(),
        _ => req.method.clone(),
    };
    let mut headers: Vec<(String, String)> = req
        .headers
        .iter()
        .filter(|(k, _)| {
            !matches!(
                k.as_str(),
                "host" | "connection" | "content-length" | "transfer-encoding"
            )
        })
        .cloned()
        .collect();
    headers.extend(mapped_headers(route, req, &Value::Null, found));
    let out = Out {
        headers,
        body: req.body.clone(),
        skip_verification: skips_verification(&route.integration),
        ..Out::new(
            method,
            url_of(found, route, req, &Value::Null, true),
            timeout(route),
        )
    };
    match call(&out).await {
        Ok(a) => reply::of(a.status, a.headers, a.body),
        // A refused certificate is a configuration error, as AWS's "General SSLEngine problem" is (memory).
        Err(Failure::Tls(why)) => {
            super::custom::config_error(found, req, &serde_json::json!({}), &why)
        }
        Err(Failure::Timeout(_)) => reply::gateway(
            found,
            "INTEGRATION_TIMEOUT",
            504,
            "Endpoint request timed out",
            req,
            None,
        ),
        Err(why) => {
            tracing::warn!(
                "execute-api {}: HTTP integration failed: {why}",
                found.api_id()
            );
            reply::gateway(
                found,
                "INTEGRATION_FAILURE",
                500,
                "Internal server error",
                req,
                None,
            )
        }
    }
}

/// An HTTP (non-proxy) integration: the mapped body sent, the answer handed to the integration responses.
pub(crate) async fn send(
    found: &Found,
    route: &Route,
    req: &Incoming,
    mapped: &Mapped,
) -> Result<Backend, String> {
    let mut headers = mapped_headers(route, req, &mapped.context, found);
    if !headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
    {
        headers.push((
            "Content-Type".into(),
            req.header("content-type")
                .unwrap_or("application/json")
                .to_owned(),
        ));
    }
    let out = Out {
        headers,
        body: Bytes::from(mapped.body.clone()),
        skip_verification: skips_verification(&route.integration),
        ..Out::new(
            text(&route.integration, "httpMethod")
                .unwrap_or("POST")
                .to_owned(),
            url_of(found, route, req, &mapped.context, false),
            timeout(route),
        )
    };
    let a = call(&out).await.map_err(|e| match e {
        Failure::Timeout(_) => "Endpoint request timed out".to_owned(),
        other => other.to_string(),
    })?;
    Ok(Backend {
        selector: a.status.to_string(),
        headers: a.headers,
        body: String::from_utf8_lossy(&a.body).into_owned(),
    })
}
