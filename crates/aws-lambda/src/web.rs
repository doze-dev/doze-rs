//! Function URLs: an HTTP request to `<id>.lambda-url.<region>.<host>`, AWS's address with the gateway's host in place of
//! `on.aws`, becomes the payload-format-2.0 event, the function runs, and its answer is turned back into an HTTP response by AWS's
//! rule: an object with a `statusCode` is the response, anything else is the body of a 200. **doze's local address**
//! `/_aws/lambda-url/<region>/<id>/<path>` on the gateway's own host serves the same URL to a client that cannot set a host or
//! resolve `*.localhost`; it is not an AWS address.
//!
//! **`AuthType` `AWS_IAM`**: the request must be signed with SigV4 for `lambda` in the URL's region, as on AWS. The gateway checks
//! the signature before the request gets here (`aws-gateway`'s `websig.rs`): under `--auth verify` always, under `--auth trust`
//! whenever the key's secret is known (the configured key, a key IAM issued), so a wrong signature is refused in AWS's words; a key
//! nothing here knows is taken on trust, as every API call is under trust, once its scope and time are right. An unsigned request
//! is `403 Forbidden` whatever the IAM mode. The signer is then asked for `lambda:InvokeFunctionUrl` by IAM (enforced, or logged under
//! `--iam soft`). **`NONE`** answers `403 Forbidden` when IAM is enforcing and the function's resource policy does not let anyone
//! invoke through the URL (`resolve::anyone_may`); `--iam soft` logs and runs it.

use std::sync::Arc;

use aws_core::base64;
use aws_wire::service::{Caller, WebReply, WebRequest, WebSignature};
use bytes::Bytes;
use serde_json::{Value, json};

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

/// Whether a path segment is a region (`us-east-1`) and not a URL id (32 letters and digits, no dash).
fn is_region(segment: &str) -> bool {
    segment.contains('-') && segment.ends_with(|c: char| c.is_ascii_digit())
}

/// Whether the request is for a function URL (not a package download): one whose signature the gateway checks for `lambda`.
pub(crate) fn is_url(host: &str, path: &str) -> bool {
    owns(host, path) && !path.starts_with(CODE_PREFIX)
}

/// The region and URL id a request is for, and the path the function is given.
fn target(host: &str, path: &str) -> Option<(String, String, String)> {
    if let Some(rest) = path.strip_prefix(PATH_PREFIX) {
        let mut parts = rest.splitn(3, '/');
        let first = parts.next()?;
        if !is_region(first) {
            return None;
        }
        let id = parts.next()?;
        return Some((
            first.to_owned(),
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

/// A refusal of a signature that is there but wrong: AWS words it as the JSON services do, under a lower-case `message`
/// (recorded: test_lambda.py::test_function_url_with_iam_auth_refuses_a_bad_signature), where an unsigned request gets `Message`.
fn refused(status: u16, message: &str) -> WebReply {
    WebReply {
        status,
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: Bytes::from(json!({ "message": message }).to_string()),
    }
}

pub(crate) async fn serve(inner: Arc<Inner>, req: WebRequest) -> WebReply {
    if req.path.starts_with(CODE_PREFIX) {
        return download(&inner, &req.path).await;
    }
    let Some((region, id, path)) = target(&req.host, &req.path) else {
        return plain(404, "Not Found");
    };
    let Some((region, found)) = find_url(&inner, &region, &id).await else {
        return plain(404, "Not Found");
    };
    let text = |k: &str| {
        found
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    if text("auth") == "AWS_IAM"
        && let Some(refusal) = signature_refusal(&req.signature, &region)
    {
        return refusal;
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

/// The URL record of `id` in `region`.
async fn find_url(inner: &Inner, region: &str, id: &str) -> Option<(String, Value)> {
    match inner.ask(region, Ask::Url(id.to_owned())).await {
        Ok(Answer::Value(Some(v))) => Some((region.to_owned(), v)),
        _ => None,
    }
}

/// Why an `AWS_IAM` URL refuses the request, if it does: no signature (`Forbidden`, recorded), a signature the gateway refused
/// (its words, AWS's for the JSON services, recorded), or one scoped to another region (memory).
fn signature_refusal(signature: &WebSignature, region: &str) -> Option<WebReply> {
    match signature {
        WebSignature::Unsigned => Some(plain(403, "Forbidden")),
        WebSignature::Refused(why) => Some(refused(403, why)),
        WebSignature::Signed { region: scoped, .. } if scoped != region => {
            // (memory)
            Some(refused(
                403,
                &format!(
                    "Credential should be scoped to a valid region. (doze-aws: the URL is in {region}, the signature is scoped to {scoped})"
                ),
            ))
        }
        WebSignature::Signed { .. } => None,
    }
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
    // An `AWS_IAM` URL is called by whoever signed; a `NONE` one by anyone.
    let (access_key_id, session_token) = match (&req.signature, text("auth").as_str()) {
        (
            WebSignature::Signed {
                access_key_id,
                session_token,
                ..
            },
            "AWS_IAM",
        ) => (access_key_id.clone(), session_token.clone()),
        _ => (Caller::ANONYMOUS.to_owned(), None),
    };
    let caller = Caller {
        access_key_id,
        account_id: account,
        region: region.to_owned(),
        endpoint: req.endpoint.clone(),
        service: None,
        session_token,
    };
    let event = crate::url_event::event_of(req, path, id, region);
    let qualifier = Some(text("qualifier")).filter(|q| !q.is_empty());
    let ask = Resolving {
        caller: caller.clone(),
        function: text("name"),
        qualifier,
        source_arn: None,
        draw: 0.0,
        url: true,
        // `NONE` is judged by the function's resource policy (anyone may call only when it says so); `AWS_IAM` by the signer's
        // policies and the function's, for `lambda:InvokeFunctionUrl`.
        internal: false,
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
        // A path with no region is not an address.
        let id = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4";
        assert_eq!(
            target("127.0.0.1:4566", &format!("/_aws/lambda-url/{id}/orders/7")),
            None
        );
        assert!(is_url("127.0.0.1:4566", "/_aws/lambda-url/us-east-1/abc/x"));
        assert!(!is_url("127.0.0.1:4566", "/_aws/lambda/code/us-east-1/abc"));
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
