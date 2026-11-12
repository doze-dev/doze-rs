//! Function URLs: an HTTP request to `<id>.lambda-url.<region>.<host>` (or to `/_aws/lambda-url/<region>/<id>/` for a client that
//! cannot set a host) becomes the payload-format-2.0 event, the function runs, and its answer is turned back into an HTTP response
//! by AWS's rule: an object with a `statusCode` is the response, anything else is the body of a 200.
//!
//! A function URL with `AuthType` `AWS_IAM` refuses a request that carries no signature when IAM is enforcing; a signed request is
//! let through without its signature being checked.

use std::sync::Arc;

use aws_core::base64;
use aws_peers::Mode;
use aws_wire::service::{Caller, WebReply, WebRequest};
use bytes::Bytes;
use serde_json::{Map, Value, json};

use crate::Inner;
use crate::core::{Answer, Ask};
use crate::invoke::Input;
use crate::resolve::Resolving;

const PATH_PREFIX: &str = "/_aws/lambda-url/";

/// Whether the request is for a function URL.
pub(crate) fn owns(host: &str, path: &str) -> bool {
    host.contains(".lambda-url.") || path.starts_with(PATH_PREFIX) || path.starts_with(CODE_PREFIX)
}

/// Where the packages of functions and layers can be downloaded from (what `Code.Location` and `Content.Location` name).
const CODE_PREFIX: &str = "/_aws/lambda/code/";

/// The package for `/_aws/lambda/code/<region>/<id>`: the zip as it was uploaded.
async fn download(inner: &Inner, path: &str) -> WebReply {
    let mut parts = path.trim_start_matches(CODE_PREFIX).splitn(2, '/');
    let (region, id) = (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    );
    let safe = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if !safe(region) || !safe(id) {
        return plain(404, "Not Found");
    }
    let file = inner
        .region_dir(region)
        .join("code")
        .join(id)
        .join("code.zip");
    match tokio::fs::read(file).await {
        Ok(bytes) => WebReply {
            status: 200,
            headers: vec![("content-type".to_owned(), "application/zip".to_owned())],
            body: Bytes::from(bytes),
        },
        Err(_) => plain(404, "Not Found"),
    }
}

/// The region and URL id a request is for.
fn target(host: &str, path: &str) -> Option<(String, String, String)> {
    if let Some(rest) = path.strip_prefix(PATH_PREFIX) {
        let mut parts = rest.splitn(3, '/');
        let (region, id) = (parts.next()?, parts.next()?);
        return Some((
            region.to_owned(),
            id.to_owned(),
            format!("/{}", parts.next().unwrap_or_default()),
        ));
    }
    let name = host.rsplit_once(':').map_or(host, |(h, p)| {
        if p.bytes().all(|b| b.is_ascii_digit()) {
            h
        } else {
            host
        }
    });
    let mut labels = name.split('.');
    let id = labels.next()?;
    (labels.next() == Some("lambda-url")).then_some(())?;
    let region = labels.next()?;
    Some((region.to_owned(), id.to_owned(), path.to_owned()))
}

fn plain(status: u16, message: &str) -> WebReply {
    WebReply {
        status,
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: Bytes::from(json!({"Message": message}).to_string()),
    }
}

pub(crate) async fn serve(inner: Arc<Inner>, req: WebRequest) -> WebReply {
    if req.path.starts_with(CODE_PREFIX) {
        return download(&inner, &req.path).await;
    }
    let Some((region, id, path)) = target(&req.host, &req.path) else {
        return plain(404, "Not Found");
    };
    let found = match inner.ask(&region, Ask::Url(id.clone())).await {
        Ok(Answer::Value(Some(v))) => v,
        _ => return plain(404, "Not Found"),
    };
    let text = |k: &str| {
        found
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let signed = req.headers.iter().any(|(k, _)| k == "authorization");
    let enforcing = inner
        .peers
        .as_ref()
        .is_some_and(|p| p.mode() == Mode::Enforce);
    if text("auth") == "AWS_IAM" && enforcing && !signed {
        return plain(403, "Forbidden");
    }
    let cors = found
        .get("cors")
        .and_then(Value::as_str)
        .and_then(|c| serde_json::from_str::<Value>(c).ok());
    if req.method == "OPTIONS"
        && let Some(cors) = &cors
    {
        return WebReply {
            status: 200,
            headers: cors_headers(cors, &req),
            body: Bytes::new(),
        };
    }
    let mut reply = run(&inner, &req, &found, (&region, &id, &path)).await;
    if let Some(cors) = &cors {
        reply.headers.extend(
            cors_headers(cors, &req)
                .into_iter()
                .filter(|(k, _)| k == "access-control-allow-origin"),
        );
    }
    reply
}

/// Runs the function a URL belongs to for the request, and turns its answer into the response.
async fn run(
    inner: &Arc<Inner>,
    req: &WebRequest,
    found: &Value,
    at: (&str, &str, &str),
) -> WebReply {
    let (region, id, path) = at;
    let text = |k: &str| {
        found
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let account = inner.account_of(region);
    let caller = Caller {
        access_key_id: "anonymous".to_owned(),
        account_id: account,
        region: region.to_owned(),
        endpoint: req.endpoint.clone(),
        service: None,
        session_token: None,
    };
    let event = event_of(req, path, id, region);
    let qualifier = Some(text("qualifier")).filter(|q| !q.is_empty());
    let ask = Resolving {
        caller: caller.clone(),
        function: text("name"),
        qualifier,
        source_arn: None,
        draw: 0.0,
        url: true,
        internal: true,
    };
    let rs = match inner.ask(region, Ask::Resolve(Box::new(ask))).await {
        Ok(Answer::Resolved(Ok(rs))) => rs,
        Ok(Answer::Resolved(Err(e))) => return plain(e.status, &e.message),
        _ => return plain(502, "Internal Server Error"),
    };
    let input = Input {
        payload: Bytes::from(event.to_string()),
        client_context: None,
        trace_id: String::new(),
    };
    let out = match inner.run_function(&rs, &caller, input).await {
        Ok(out) => out,
        Err(e) if e.status == 429 => return plain(429, "Rate Exceeded."),
        Err(_) => return plain(502, "Internal Server Error"),
    };
    if out.error {
        plain(502, "Internal Server Error")
    } else {
        response_of(&out.payload)
    }
}

fn cors_headers(cors: &Value, req: &WebRequest) -> Vec<(String, String)> {
    let origin = req
        .headers
        .iter()
        .find(|(k, _)| k == "origin")
        .map(|(_, v)| v.clone());
    let list = |k: &str| -> Vec<String> {
        cors.get(k)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    let allowed = list("AllowOrigins");
    let mut out = Vec::new();
    if let Some(o) = origin.filter(|o| allowed.iter().any(|a| a == "*" || a == o)) {
        out.push(("access-control-allow-origin".to_owned(), o));
        out.push((
            "access-control-allow-methods".to_owned(),
            list("AllowMethods").join(", "),
        ));
        out.push((
            "access-control-allow-headers".to_owned(),
            list("AllowHeaders").join(", "),
        ));
    }
    out
}

/// The payload-format-2.0 event for a request.
fn event_of(req: &WebRequest, path: &str, id: &str, region: &str) -> Value {
    let mut headers = Map::new();
    for (k, v) in &req.headers {
        headers
            .entry(k.clone())
            .and_modify(|old| {
                if let Value::String(s) = old {
                    s.push(',');
                    s.push_str(v);
                }
            })
            .or_insert_with(|| Value::String(v.clone()));
    }
    let mut query = Map::new();
    for pair in req.query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let (k, v) = (decode(k), decode(v));
        query
            .entry(k)
            .and_modify(|old| {
                if let Value::String(s) = old {
                    s.push(',');
                    s.push_str(&v);
                }
            })
            .or_insert(Value::String(v));
    }
    let cookies: Vec<&str> = req
        .headers
        .iter()
        .filter(|(k, _)| k == "cookie")
        .flat_map(|(_, v)| v.split("; "))
        .collect();
    let (body, encoded) = match std::str::from_utf8(&req.body) {
        Ok(text) => (text.to_owned(), false),
        Err(_) => (base64::encode(&req.body), true),
    };
    let now = aws_core::time::format_http_date(0);
    let _ = now;
    let ua = headers.get("user-agent").cloned().unwrap_or(Value::Null);
    let mut event = json!({
        "version": "2.0",
        "routeKey": "$default",
        "rawPath": path,
        "rawQueryString": req.query,
        "headers": headers,
        "requestContext": {
            "accountId": "anonymous",
            "apiId": id,
            "domainName": req.host,
            "domainPrefix": id,
            "http": {"method": req.method, "path": path, "protocol": "HTTP/1.1", "sourceIp": "127.0.0.1", "userAgent": ua},
            "requestId": aws_seal::uuid_v4().unwrap_or_default(),
            "routeKey": "$default",
            "stage": "$default",
            "region": region,
        },
        "isBase64Encoded": encoded,
    });
    if !query.is_empty() {
        crate::set(&mut event, "queryStringParameters", Value::Object(query));
    }
    if !cookies.is_empty() {
        crate::set(&mut event, "cookies", json!(cookies));
    }
    if !body.is_empty() {
        crate::set(&mut event, "body", Value::String(body));
    }
    event
}

fn decode(s: &str) -> String {
    let spaced = s.replace('+', " ");
    aws_core::uri::decode_str(&spaced).map_or(spaced.clone(), std::borrow::Cow::into_owned)
}

/// The HTTP response for what a function returned.
fn response_of(payload: &[u8]) -> WebReply {
    let value: Value = serde_json::from_slice(payload).unwrap_or(Value::Null);
    let Some(obj) = value.as_object().filter(|o| o.contains_key("statusCode")) else {
        let body = if value.is_null() && payload.is_empty() {
            Bytes::new()
        } else {
            Bytes::copy_from_slice(payload)
        };
        return WebReply {
            status: 200,
            headers: vec![("content-type".to_owned(), "application/json".to_owned())],
            body,
        };
    };
    let status = obj
        .get("statusCode")
        .and_then(Value::as_u64)
        .and_then(|s| u16::try_from(s).ok())
        .unwrap_or(200);
    let mut headers: Vec<(String, String)> = obj
        .get("headers")
        .and_then(Value::as_object)
        .map(|h| {
            h.iter()
                .filter_map(|(k, v)| Some((k.to_lowercase(), v.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    for c in obj
        .get("cookies")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        headers.push(("set-cookie".to_owned(), c.to_owned()));
    }
    let raw = obj.get("body").and_then(Value::as_str).unwrap_or_default();
    let body = if obj.get("isBase64Encoded").and_then(Value::as_bool) == Some(true) {
        Bytes::from(base64::decode(raw).unwrap_or_default())
    } else {
        Bytes::from(raw.to_owned())
    };
    if !headers.iter().any(|(k, _)| k == "content-type") {
        headers.push(("content-type".to_owned(), "application/json".to_owned()));
    }
    WebReply {
        status,
        headers,
        body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_is_found_by_host_or_by_path() {
        assert!(owns("abc.lambda-url.us-east-1.localhost:4566", "/x"));
        assert!(owns("127.0.0.1:4566", "/_aws/lambda-url/us-east-1/abc/x"));
        assert!(!owns("127.0.0.1:4566", "/2015-03-31/functions"));
        assert_eq!(
            target("abc.lambda-url.eu-west-1.localhost:4566", "/orders/7"),
            Some(("eu-west-1".into(), "abc".into(), "/orders/7".into()))
        );
        assert_eq!(
            target("127.0.0.1:4566", "/_aws/lambda-url/us-east-1/abc/orders"),
            Some(("us-east-1".into(), "abc".into(), "/orders".into()))
        );
    }

    #[test]
    fn a_status_code_object_is_the_response_and_anything_else_is_a_body() {
        let r = response_of(
            br#"{"statusCode":201,"headers":{"X-A":"b"},"body":"hi","cookies":["a=1"]}"#,
        );
        assert_eq!((r.status, r.body.as_ref()), (201, b"hi".as_slice()));
        assert!(r.headers.contains(&("x-a".to_owned(), "b".to_owned())));
        assert!(
            r.headers
                .contains(&("set-cookie".to_owned(), "a=1".to_owned()))
        );
        let bare = response_of(br#"{"n":1}"#);
        assert_eq!(
            (bare.status, bare.body.as_ref()),
            (200, br#"{"n":1}"#.as_slice())
        );
    }
}
