//! The HTTP Task (`arn:aws:states:::http:invoke`): one HTTP/1.1 request to `ApiEndpoint` with `Method`, `Headers`,
//! `QueryParameters` and `RequestBody` (JSON as JSON, a string as it is). The answer is `{Headers, ResponseBody, StatusCode,
//! StatusText}`; a status that is not 2xx fails the state as `States.Http.StatusCode.<code>`. The request is made with the client the
//! other services share (`aws-webhook`): `http://` and `https://` endpoints (a certificate the machine trusts, or `--ca-file`), at
//! most 60 seconds, and within the process's cap on outbound requests. The EventBridge connection's credentials are not added (its
//! secret is not readable over the API).

use std::sync::Arc;
use std::time::Duration;

use aws_asl::Failure;
use aws_peers::Peers;
use aws_webhook::{Failure as Refused, Request};
use aws_wire::service::Caller;
use serde_json::{Map, Value, json};

use super::not_local_failure;

const RESOURCE: &str = "arn:aws:states:::http:invoke";
const LIMIT: Duration = Duration::from_secs(60);
/// The largest answer read (AWS's payload quota, and room for the head).
const MOST: usize = 262_144 + 65_536;

pub(super) async fn invoke(
    _peers: &Arc<Peers>,
    _caller: &Caller,
    input: &Value,
) -> Result<Value, Failure> {
    let endpoint = input
        .get("ApiEndpoint")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let method = input
        .get("Method")
        .and_then(Value::as_str)
        .unwrap_or("GET")
        .to_uppercase();
    if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
        return Err(not_local_failure(
            RESOURCE,
            "ApiEndpoint is not an http:// or https:// URL",
        ));
    }
    let url = with_query(endpoint, input.get("QueryParameters"));
    let body = match input.get("RequestBody") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    };
    let mut headers: Vec<(String, String)> = match input.get("Headers") {
        // The client writes these itself.
        Some(Value::Object(h)) => h
            .iter()
            .filter(|(k, _)| {
                !["host", "content-length", "connection"].contains(&k.to_ascii_lowercase().as_str())
            })
            .map(|(k, v)| (k.clone(), super::as_text(v)))
            .collect(),
        _ => Vec::new(),
    };
    if !body.is_empty()
        && !headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
    {
        headers.push(("Content-Type".to_owned(), "application/json".to_owned()));
    }
    let request = Request {
        headers: &headers,
        body: body.as_bytes(),
        timeout: LIMIT,
        limit: MOST,
        ..Request::new(&method, &url)
    };
    match aws_webhook::fetch(&request).await {
        Ok(a) => answer(a.status, &a.reason, &a.headers, &a.body),
        Err(Refused::Timeout(_)) => Err(Failure::new(
            "States.Timeout",
            format!("{endpoint} did not answer within 60 seconds"),
        )),
        Err(e) => Err(Failure::new(
            "States.Http.Socket",
            format!("{endpoint}: {e}"),
        )),
    }
}

fn with_query(path: &str, q: Option<&Value>) -> String {
    let Some(Value::Object(q)) = q else {
        return path.to_owned();
    };
    let pairs: Vec<String> = q
        .iter()
        .map(|(k, v)| {
            let v = super::as_text(v);
            format!(
                "{}={}",
                aws_core::uri::encode(k.as_bytes(), false),
                aws_core::uri::encode(v.as_bytes(), false)
            )
        })
        .collect();
    let sep = if path.contains('?') { '&' } else { '?' };
    format!("{path}{sep}{}", pairs.join("&"))
}

fn answer(
    code: u16,
    reason: &str,
    head: &[(String, String)],
    body: &[u8],
) -> Result<Value, Failure> {
    let mut headers = Map::new();
    for (k, v) in head {
        headers.insert(k.clone(), json!([v]));
    }
    let body = String::from_utf8_lossy(body).into_owned();
    let parsed = serde_json::from_str(&body).unwrap_or(Value::String(body));
    let out = json!({"Headers": headers, "ResponseBody": parsed, "StatusCode": code, "StatusText": reason});
    if (200..300).contains(&code) {
        Ok(out)
    } else {
        // (memory)
        Err(Failure::new(
            format!("States.Http.StatusCode.{code}"),
            out.to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A server that answers each request with `status` and a JSON body, and keeps what it was sent.
    async fn server(status: &'static str) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let keep = Arc::clone(&seen);
        tokio::spawn(async move {
            while let Ok((mut s, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).await.unwrap();
                keep.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buf[..n]).into_owned());
                let body = r#"{"ok":true}"#;
                let a = format!(
                    "HTTP/1.1 {status}\r\nX-Seen: 1\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n",
                    body.len()
                );
                s.write_all(a.as_bytes()).await.unwrap();
            }
        });
        (base, seen)
    }

    #[tokio::test]
    async fn the_request_is_made_and_the_answer_is_the_states_output() {
        let (base, seen) = server("201 Created").await;
        let input = json!({"ApiEndpoint": format!("{base}/orders"), "Method": "post", "QueryParameters": {"a": "x y"},
            "Headers": {"X-Mine": "1", "Host": "elsewhere"}, "RequestBody": {"n": 1}});
        let out = invoke(&Peers::new(), &caller(), &input).await.unwrap();
        assert_eq!(out["StatusCode"], 201);
        assert_eq!(out["StatusText"], "Created");
        assert_eq!(out["ResponseBody"], json!({"ok": true}));
        assert_eq!(out["Headers"]["X-Seen"], json!(["1"]));
        let sent = seen.lock().unwrap()[0].clone();
        assert!(
            sent.starts_with("POST /orders?a=x%20y HTTP/1.1\r\n"),
            "{sent}"
        );
        assert!(
            sent.contains("X-Mine: 1\r\n") && sent.contains("Content-Type: application/json\r\n")
        );
        assert!(
            !sent.contains("elsewhere"),
            "the client writes Host itself: {sent}"
        );
        assert!(sent.ends_with(r#"{"n":1}"#), "{sent}");
    }

    #[tokio::test]
    async fn a_status_that_is_not_2xx_fails_and_https_is_called() {
        let (base, _) = server("404 Not Found").await;
        let e = invoke(&Peers::new(), &caller(), &json!({"ApiEndpoint": base}))
            .await
            .unwrap_err();
        assert_eq!(e.error, "States.Http.StatusCode.404");
        // https is no longer refused as "not local": a port nothing listens on is a socket failure.
        let e = invoke(
            &Peers::new(),
            &caller(),
            &json!({"ApiEndpoint": "https://127.0.0.1:1/"}),
        )
        .await
        .unwrap_err();
        assert_eq!(e.error, "States.Http.Socket");
    }

    fn caller() -> Caller {
        Caller {
            access_key_id: "test".to_owned(),
            account_id: "000000000000".to_owned(),
            region: "us-east-1".to_owned(),
            endpoint: String::new(),
            service: None,
            session_token: None,
        }
    }
}
