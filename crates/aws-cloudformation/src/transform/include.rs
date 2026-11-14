//! Templates and snippets kept in the local S3: a `TemplateURL`, a nested stack's template, and `AWS::Include`.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use aws_peers::Peers;
use aws_wire::ApiError;
use aws_wire::service::Caller;

use crate::errors::validation;

/// The largest template read from S3 (AWS: 1 MB).
const MAX_TEMPLATE: usize = 1_000_000;

/// The bucket and key an S3 URL names: virtual-hosted (`https://b.s3.region.amazonaws.com/k`), path-style
/// (`https://s3.region.amazonaws.com/b/k`, or the local endpoint `http://127.0.0.1:4566/b/k`), or `s3://b/k`.
pub(crate) fn bucket_and_key(url: &str) -> Option<(String, String)> {
    let decode = |s: &str| aws_core::uri::decode_str(s).map(|c| c.into_owned());
    if let Some(rest) = url.strip_prefix("s3://") {
        let (b, k) = rest.split_once('/')?;
        return Some((b.to_owned(), decode(k)?));
    }
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let rest = rest.split(['?', '#']).next()?;
    let (host, path) = rest.split_once('/')?;
    let host = host.split(':').next()?;
    let virtual_hosted = host
        .find(".s3.")
        .or_else(|| host.find(".s3-"))
        .filter(|_| host.ends_with(".amazonaws.com") || host.ends_with(".amazonaws.com.cn"));
    if let Some(i) = virtual_hosted {
        return Some((host.get(..i)?.to_owned(), decode(path)?));
    }
    let (b, k) = path.split_once('/')?;
    if b.is_empty() || k.is_empty() {
        return None;
    }
    Some((b.to_owned(), decode(k)?))
}

/// The bytes of an object in the local S3, read as the caller.
///
/// # Errors
/// Why it could not be read.
pub(crate) async fn object(
    peers: Option<&Peers>,
    caller: &Caller,
    bucket: &str,
    key: &str,
    max: usize,
) -> Result<Vec<u8>, String> {
    let peers = peers.ok_or("S3 is not running")?;
    let mut reply = peers
        .call_rest(
            "cloudformation",
            caller,
            "s3",
            "GetObject",
            &json!({"Bucket": bucket, "Key": key}),
        )
        .await
        .map_err(|e| format!("{}: {}", e.code, e.message))?;
    let Some(mut stream) = reply.body.take() else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    while let Some(chunk) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        out.extend_from_slice(&chunk);
        if out.len() > max {
            return Err(format!("larger than {max} bytes"));
        }
    }
    Ok(out)
}

/// The text of a template at `url`.
///
/// # Errors
/// `ValidationError` for a URL that names no object of the local S3.
pub(crate) async fn fetch(
    peers: Option<&Peers>,
    caller: &Caller,
    url: &str,
) -> Result<String, ApiError> {
    let Some((bucket, key)) = bucket_and_key(url) else {
        // (memory)
        return Err(validation("TemplateURL must be a supported URL."));
    };
    let bytes = object(peers, caller, &bucket, &key, MAX_TEMPLATE)
        .await
        // (memory)
        .map_err(|e| validation(format!("S3 error: {e}")))?;
    String::from_utf8(bytes)
        .map_err(|_| validation("Template format error: unsupported structure.")) // (memory)
}

/// Every `Fn::Transform` of `AWS::Include` in `v` replaced by the snippet it names (a snippet in place of a value, or merged into
/// the object that holds it), and a top-level `Transform: AWS::Include` likewise.
pub(crate) async fn expand(
    v: &mut Value,
    peers: Option<&Peers>,
    caller: &Caller,
    params: &BTreeMap<String, Value>,
    depth: usize,
) -> Result<(), ApiError> {
    if depth > 32 {
        return Err(validation(
            "Template format error: AWS::Include is nested too deeply",
        )); // (memory)
    }
    if let Value::Object(o) = v
        && let Some(t) = o.get("Fn::Transform").cloned()
    {
        let snippet = snippet(&t, peers, caller, params).await?;
        o.remove("Fn::Transform");
        if o.is_empty() {
            *v = snippet;
        } else if let Value::Object(more) = snippet {
            o.extend(more);
        }
        return Box::pin(expand(v, peers, caller, params, depth + 1)).await;
    }
    match v {
        Value::Object(o) => {
            for x in o.values_mut() {
                Box::pin(expand(x, peers, caller, params, depth + 1)).await?;
            }
        }
        Value::Array(a) => {
            for x in a {
                Box::pin(expand(x, peers, caller, params, depth + 1)).await?;
            }
        }
        _ => {}
    }
    Ok(())
}

async fn snippet(
    t: &Value,
    peers: Option<&Peers>,
    caller: &Caller,
    params: &BTreeMap<String, Value>,
) -> Result<Value, ApiError> {
    let name = t.get("Name").and_then(Value::as_str).unwrap_or_default();
    if name != "AWS::Include" {
        // (memory)
        return Err(validation(format!(
            "Template format error: No transform named {name} found."
        )));
    }
    let location = match t.get("Parameters").and_then(|p| p.get("Location")) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Object(o)) if o.contains_key("Ref") => o
            .get("Ref")
            .and_then(Value::as_str)
            .and_then(|r| params.get(r))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_default(),
        _ => String::new(),
    };
    let text = fetch(peers, caller, &location).await?;
    crate::tmpl::read_snippet(&text)
}
