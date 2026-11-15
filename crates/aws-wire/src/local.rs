//! doze's own addresses (`/_doze/...`): local helpers that are not AWS endpoints (`/_doze/data`, `/_doze/iam/...`,
//! `/_doze/sqs/...`), and the one guard they share.
//!
//! They answer without a signature, so they are guarded against the two ways a stranger could reach them. A web page the person
//! has open could send to `127.0.0.1`: a `POST` must be `application/json` (which a page cannot send across origins without a
//! preflight, and nothing here answers one), and an `Origin`, when sent, must be this server. And under `--auth verify` the server
//! may be reachable from elsewhere: every request must then carry the configured secret key in `X-Doze-Key`.

use crate::service::{WebReply, WebRequest};

/// The header that carries the secret key under `--auth verify`.
pub const KEY_HEADER: &str = "x-doze-key";

/// The value of header `name` (lower-case), if the request has it.
#[must_use]
pub fn header<'a>(req: &'a WebRequest, name: &str) -> Option<&'a str> {
    req.headers
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

/// A JSON answer that no cache keeps.
#[must_use]
pub fn reply(status: u16, body: &serde_json::Value) -> WebReply {
    WebReply {
        status,
        headers: vec![
            ("content-type".to_owned(), "application/json".to_owned()),
            ("cache-control".to_owned(), "no-store".to_owned()),
        ],
        body: serde_json::to_vec(body).unwrap_or_default().into(),
    }
}

fn error(status: u16, message: &str) -> WebReply {
    reply(status, &serde_json::json!({ "error": message }))
}

/// Why a request to a local address may not be answered, if it may not. `key` is the configured secret key when the server
/// verifies signatures (`--auth verify`), `None` when it takes them on trust.
#[must_use]
pub fn guard(req: &WebRequest, key: Option<&str>) -> Option<WebReply> {
    if let Some(key) = key
        && header(req, KEY_HEADER) != Some(key)
    {
        return Some(error(
            403,
            "this doze-aws verifies signatures: send the configured secret key in X-Doze-Key",
        ));
    }
    if let Some(origin) = header(req, "origin") {
        let host = origin.split("://").nth(1).unwrap_or(origin);
        if host != req.host {
            return Some(error(
                403,
                "a page from another origin may not use doze's /_doze/ addresses",
            ));
        }
    }
    let json_body = header(req, "content-type").is_some_and(|t| t.starts_with("application/json"));
    (req.method == "POST" && !json_body).then(|| error(415, "send the request as application/json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: &str, headers: &[(&str, &str)]) -> WebRequest {
        WebRequest {
            method: method.to_owned(),
            host: "127.0.0.1:4566".to_owned(),
            path: "/_doze/iam/DozeAccessLog".to_owned(),
            query: String::new(),
            headers: headers
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            body: bytes::Bytes::new(),
            endpoint: "http://127.0.0.1:4566".to_owned(),
        }
    }

    fn status(r: Option<WebReply>) -> Option<u16> {
        r.map(|r| r.status)
    }

    #[test]
    fn the_key_is_required_only_when_one_is_configured() {
        assert_eq!(status(guard(&request("GET", &[]), None)), None);
        assert_eq!(status(guard(&request("GET", &[]), Some("s"))), Some(403));
        let wrong = request("GET", &[(KEY_HEADER, "t")]);
        assert_eq!(status(guard(&wrong, Some("s"))), Some(403));
        let right = request("GET", &[(KEY_HEADER, "s")]);
        assert_eq!(status(guard(&right, Some("s"))), None);
    }

    #[test]
    fn another_origin_and_a_form_post_are_refused() {
        let page = request("GET", &[("origin", "https://evil.example")]);
        assert_eq!(status(guard(&page, None)), Some(403));
        let own = request("GET", &[("origin", "http://127.0.0.1:4566")]);
        assert_eq!(status(guard(&own, None)), None);
        let form = request(
            "POST",
            &[("content-type", "application/x-www-form-urlencoded")],
        );
        assert_eq!(status(guard(&form, None)), Some(415));
        let json = request("POST", &[("content-type", "application/json")]);
        assert_eq!(status(guard(&json, None)), None);
    }
}
