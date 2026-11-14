//! The HTTP Task (`arn:aws:states:::http:invoke`): one HTTP/1.1 request to `ApiEndpoint` with `Method`, `Headers`,
//! `QueryParameters` and `RequestBody` (JSON as JSON, a string as it is). The answer is `{Headers, ResponseBody, StatusCode,
//! StatusText}`; a status that is not 2xx fails the state as `States.Http.StatusCode.<code>`. Only `http://` endpoints are called
//! (doze-aws links no TLS client), and the EventBridge connection's credentials are not added (its secret is not readable over the
//! API): both are said in the failure or the log, not hidden.

use std::sync::Arc;
use std::time::Duration;

use aws_asl::Failure;
use aws_peers::Peers;
use aws_wire::service::Caller;
use serde_json::{Map, Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::not_local_failure;

const RESOURCE: &str = "arn:aws:states:::http:invoke";
const LIMIT: Duration = Duration::from_secs(60);
/// The largest answer read (AWS's payload quota).
const MOST: usize = 262_144;

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
    let Some(rest) = endpoint.strip_prefix("http://") else {
        return Err(not_local_failure(
            RESOURCE,
            "only http:// endpoints are called locally (no TLS client is linked)",
        ));
    };
    let (authority, path) = rest
        .split_once('/')
        .map_or((rest, "/".to_owned()), |(a, p)| (a, format!("/{p}")));
    let path = with_query(&path, input.get("QueryParameters"));
    let body = match input.get("RequestBody") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    };
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    if let Some(Value::Object(h)) = input.get("Headers") {
        for (k, v) in h {
            head.push_str(&format!("{k}: {}\r\n", super::as_text(v)));
        }
    }
    if !body.is_empty() && !head.to_ascii_lowercase().contains("content-type:") {
        head.push_str("Content-Type: application/json\r\n");
    }
    head.push_str("\r\n");
    let raw = tokio::time::timeout(LIMIT, exchange(authority, head, body))
        .await
        .map_err(|_| {
            Failure::new(
                "States.Timeout",
                format!("{endpoint} did not answer within 60 seconds"),
            )
        })?
        .map_err(|e| Failure::new("States.Http.Socket", format!("{endpoint}: {e}")))?;
    answer(&raw)
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

async fn exchange(authority: &str, head: String, body: String) -> std::io::Result<Vec<u8>> {
    let addr = if authority.contains(':') {
        authority.to_owned()
    } else {
        format!("{authority}:80")
    };
    let mut s = tokio::net::TcpStream::connect(addr).await?;
    s.write_all(head.as_bytes()).await?;
    s.write_all(body.as_bytes()).await?;
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = s.read(&mut buf).await?;
        if n == 0 || out.len() > MOST + 65_536 {
            return Ok(out);
        }
        out.extend_from_slice(buf.get(..n).unwrap_or_default());
    }
}

fn answer(raw: &[u8]) -> Result<Value, Failure> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let mut lines = head.lines();
    let status_line = lines.next().unwrap_or_default();
    let mut parts = status_line.splitn(3, ' ');
    let code: u16 = parts.nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    let reason = parts.next().unwrap_or_default();
    let mut headers = Map::new();
    let mut chunked = false;
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            let v = v.trim();
            chunked |=
                k.eq_ignore_ascii_case("transfer-encoding") && v.eq_ignore_ascii_case("chunked");
            headers.insert(k.trim().to_owned(), json!([v]));
        }
    }
    let body = if chunked {
        unchunk(body)
    } else {
        body.to_owned()
    };
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

fn unchunk(body: &str) -> String {
    let mut out = String::new();
    let mut rest = body;
    while let Some((size, after)) = rest.split_once("\r\n") {
        let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
        if n == 0 {
            break;
        }
        out.push_str(after.get(..n).unwrap_or(after));
        rest = after
            .get(n..)
            .unwrap_or_default()
            .trim_start_matches("\r\n");
    }
    out
}
