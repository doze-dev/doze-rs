//! Calling a Lambda function from the plane (over the peer bus, as `apigateway.amazonaws.com`, with the method's execute-api ARN as
//! `SourceArn` so a permission that names it matches), and reading a proxy integration's answer by AWS's rules.

use aws_wire::service::Caller;
use bytes::Bytes;
use serde_json::{Value, json};

use crate::Inner;

/// What a function answered.
#[derive(Debug)]
pub(crate) struct Invoked {
    pub(crate) payload: Bytes,
    /// `Unhandled` or `Handled` when the function failed.
    pub(crate) error: Option<String>,
}

/// An HTTP answer built from a function's.
#[derive(Debug)]
pub(crate) struct Answer {
    pub(crate) status: u16,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Bytes,
}

/// The function ARN an integration URI names (`arn:aws:apigateway:<region>:lambda:path/2015-03-31/functions/<arn>/invocations`, or a
/// bare function ARN on an HTTP API), with `${stageVariables.x}` filled in.
pub(crate) fn function_of(
    uri: &str,
    vars: Option<&serde_json::Map<String, Value>>,
) -> Option<String> {
    let uri = fill_vars(uri, vars);
    if uri.starts_with("arn:aws:lambda:") {
        return Some(uri);
    }
    let after = uri.split_once("/functions/")?.1;
    Some(
        after
            .strip_suffix("/invocations")
            .unwrap_or(after)
            .to_owned(),
    )
}

/// `${stageVariables.name}` (and `$stageVariables.name`) replaced by the stage's value.
pub(crate) fn fill_vars(s: &str, vars: Option<&serde_json::Map<String, Value>>) -> String {
    let mut out = s.to_owned();
    for (k, v) in vars.into_iter().flatten() {
        let v = v.as_str().unwrap_or_default();
        out = out
            .replace(&format!("${{stageVariables.{k}}}"), v)
            .replace(&format!("$stageVariables.{k}"), v);
    }
    out
}

/// Invokes `function` with `event`, as API Gateway.
pub(crate) async fn invoke(
    inner: &Inner,
    account: &str,
    function: &str,
    source_arn: &str,
    event: &[u8],
) -> Result<Invoked, String> {
    let peers = inner
        .peers
        .as_ref()
        .ok_or("no Lambda is running to be called")?;
    let region = function.split(':').nth(3).unwrap_or("us-east-1");
    let caller = Caller {
        access_key_id: "apigateway".into(),
        account_id: account.to_owned(),
        region: region.to_owned(),
        endpoint: String::new(),
        service: None,
        session_token: None,
    };
    let body = json!({
        "FunctionName": function,
        "Payload": aws_core::base64::encode(event),
        "SourceArn": source_arn,
    });
    let reply = peers
        .call_rest("apigateway", &caller, "lambda", "Invoke", &body)
        .await
        .map_err(|e| format!("{}: {}", e.code, e.message))?;
    let payload = reply
        .json
        .get("Payload")
        .and_then(Value::as_str)
        .and_then(aws_core::base64::decode)
        .unwrap_or_default();
    Ok(Invoked {
        payload: Bytes::from(payload),
        error: reply
            .json
            .get("FunctionError")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

/// The members a payload-1.0 answer may have; anything else makes it malformed `(memory)`.
const V1_KEYS: &[&str] = &[
    "statusCode",
    "headers",
    "multiValueHeaders",
    "body",
    "isBase64Encoded",
    "cookies",
];

/// A proxy answer of payload format 1.0, or why it is malformed.
pub(crate) fn proxy_v1(payload: &[u8]) -> Result<Answer, String> {
    let v: Value =
        serde_json::from_slice(payload).map_err(|_| "the answer is not JSON".to_owned())?;
    let o = v.as_object().ok_or("the answer is not a JSON object")?;
    if let Some(k) = o.keys().find(|k| !V1_KEYS.contains(&k.as_str())) {
        return Err(format!(
            "the answer has a member {k} a proxy answer does not have"
        ));
    }
    let status = o
        .get("statusCode")
        .and_then(|s| {
            s.as_u64()
                .or_else(|| s.as_str().and_then(|t| t.parse().ok()))
        })
        .and_then(|s| u16::try_from(s).ok())
        .ok_or("the answer has no statusCode")?;
    structured(o, status)
}

fn structured(o: &serde_json::Map<String, Value>, status: u16) -> Result<Answer, String> {
    let mut headers = Vec::new();
    for (k, v) in o
        .get("headers")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        headers.push((k.clone(), super::mapping::plain(v)));
    }
    for (k, vs) in o
        .get("multiValueHeaders")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let list = vs
            .as_array()
            .ok_or("multiValueHeaders holds a value that is not a list")?;
        headers.retain(|(h, _)| !h.eq_ignore_ascii_case(k));
        headers.extend(list.iter().map(|x| (k.clone(), super::mapping::plain(x))));
    }
    for c in o
        .get("cookies")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        headers.push(("Set-Cookie".into(), super::mapping::plain(c)));
    }
    let text = match o.get("body") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    };
    let body = if o.get("isBase64Encoded").and_then(Value::as_bool) == Some(true) {
        aws_core::base64::decode(&text)
            .ok_or("isBase64Encoded is set and the body is not base64")?
    } else {
        text.into_bytes()
    };
    if !headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
    {
        headers.push(("Content-Type".into(), "application/json".into()));
    }
    Ok(Answer {
        status,
        headers,
        body: Bytes::from(body),
    })
}

/// A proxy answer of payload format 2.0: an object with `statusCode` is the whole answer; any other JSON is the body of a 200.
pub(crate) fn proxy_v2(payload: &[u8]) -> Result<Answer, String> {
    let v: Value =
        serde_json::from_slice(payload).map_err(|_| "the answer is not JSON".to_owned())?;
    if let Some(o) = v.as_object()
        && let Some(s) = o.get("statusCode")
    {
        let status = s
            .as_u64()
            .or_else(|| s.as_str().and_then(|t| t.parse().ok()))
            .and_then(|s| u16::try_from(s).ok())
            .ok_or("statusCode is not a number")?;
        return structured(o, status);
    }
    Ok(Answer {
        status: 200,
        headers: vec![("Content-Type".into(), "application/json".into())],
        body: Bytes::copy_from_slice(payload),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_arns_come_out_of_integration_uris() {
        let uri = "arn:aws:apigateway:us-east-1:lambda:path/2015-03-31/functions/arn:aws:lambda:us-east-1:1:function:f/invocations";
        assert_eq!(
            function_of(uri, None).as_deref(),
            Some("arn:aws:lambda:us-east-1:1:function:f")
        );
        let vars = serde_json::json!({"fn": "g"});
        assert_eq!(
            function_of(
                "arn:aws:lambda:us-east-1:1:function:${stageVariables.fn}",
                vars.as_object()
            )
            .as_deref(),
            Some("arn:aws:lambda:us-east-1:1:function:g")
        );
    }

    #[test]
    fn proxy_answers_are_read_by_the_rules_of_each_format() {
        let a = proxy_v1(br#"{"statusCode": 201, "headers": {"x": "1"}, "body": "hi"}"#).unwrap();
        assert_eq!((a.status, &a.body[..]), (201, &b"hi"[..]));
        assert!(proxy_v1(br#"{"body": "hi"}"#).is_err());
        assert!(proxy_v1(br#"{"statusCode": 200, "extra": 1}"#).is_err());
        let b = proxy_v2(br#"{"hello": "world"}"#).unwrap();
        assert_eq!(
            (b.status, &b.body[..]),
            (200, &br#"{"hello": "world"}"#[..])
        );
        let c = proxy_v2(br#"{"statusCode": 204, "cookies": ["a=1"]}"#).unwrap();
        assert!(c.headers.contains(&("Set-Cookie".into(), "a=1".into())));
    }
}
