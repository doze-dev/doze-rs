//! The event a function URL request becomes: payload format 2.0, with the body as text or as base64 by its content type.

use aws_core::base64;
use aws_wire::service::WebRequest;
use serde_json::{Map, Value, json};

/// The payload-format-2.0 event for a request.
pub(crate) fn event_of(req: &WebRequest, path: &str, id: &str, region: &str) -> Value {
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
    let (body, encoded) = body_of(req);
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

/// The body as the function sees it, and whether it is base64.
fn body_of(req: &WebRequest) -> (String, bool) {
    let content_type = req
        .headers
        .iter()
        .find(|(k, _)| k == "content-type")
        .map(|(_, v)| v.as_str());
    match std::str::from_utf8(&req.body) {
        Ok(text) if is_text(content_type) => (text.to_owned(), false),
        _ => (base64::encode(&req.body), true),
    }
}

fn decode(s: &str) -> String {
    let spaced = s.replace('+', " ");
    aws_core::uri::decode_str(&spaced).map_or(spaced.clone(), std::borrow::Cow::into_owned)
}

/// Whether a body of this type reaches the function as text. Recorded 2026-11-15: `text/plain`, `text/csv`, `application/json`
/// and `application/xml` as text; `application/octet-stream`, `application/x-www-form-urlencoded`, `image/png` and a body with no
/// type as base64. The `+json`/`+xml` suffixes and JavaScript are `(memory)`.
fn is_text(content_type: Option<&str>) -> bool {
    let Some(t) = content_type else {
        return false;
    };
    let t = t
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    t.starts_with("text/")
        || matches!(
            t.as_str(),
            "application/json" | "application/xml" | "application/javascript"
        )
        || t.ends_with("+json")
        || t.ends_with("+xml")
}

#[cfg(test)]
mod tests {
    use super::is_text;

    #[test]
    fn text_types_stay_text_and_the_rest_is_base64() {
        for t in [
            "text/plain",
            "text/csv; charset=utf-8",
            "application/json",
            "application/xml",
            "application/ld+json",
        ] {
            assert!(is_text(Some(t)), "{t}");
        }
        for t in [
            "application/octet-stream",
            "application/x-www-form-urlencoded",
            "image/png",
        ] {
            assert!(!is_text(Some(t)), "{t}");
        }
        assert!(!is_text(None));
    }
}
