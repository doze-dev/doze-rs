//! The refusal in front of every route, a `Host` the guard does not allow, and the limit on what a request may bring. (The
//! same-origin proof of a write is the API's own, `api::guard`.)

use axum::extract::{Request, State};
use axum::http::header::{CONTENT_LENGTH, HOST};
use axum::http::{HeaderName, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use kafka_auth::hostguard;

use crate::state::Shared;

/// The most a request body may be: a form is a few hundred bytes, and a 1 MiB body is refused unread.
pub const MAX_BODY: usize = 1 << 20;

fn text<'a>(req: &'a Request, name: &HeaderName) -> Option<&'a str> {
    req.headers().get(name).and_then(|v| v.to_str().ok())
}

/// Text for HTML: the five characters that could start markup or end an attribute are written as references.
#[must_use]
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// A small page for a refusal: the guard answers before anything else is built, so a person who typed the address sees why.
/// `body` is markup made here; nothing from the request goes in it unescaped.
fn refusal(status: StatusCode, title: &str, body: &str) -> Response {
    let page = format!(
        "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>{title} · doze-kafka</title></head>\
         <body style=\"font-family:system-ui,sans-serif;max-width:40rem;margin:4rem auto;padding:0 1rem;line-height:1.5\">\
         <h1>{title}</h1>{body}</body></html>",
        title = escape(title),
    );
    (
        status,
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        page,
    )
        .into_response()
}

/// 403 for a `Host` the guard does not allow, naming the one it saw and the flag that allows another.
pub async fn host_guard(State(s): State<Shared>, req: Request, next: Next) -> Response {
    let host = text(&req, &HOST).unwrap_or("");
    if hostguard::allowed(host, &s.allow_hosts) {
        return next.run(req).await;
    }
    let shown = escape(&host.chars().take(120).collect::<String>());
    refusal(
        StatusCode::FORBIDDEN,
        "This Host is not allowed",
        &format!(
            "<p>The console answered nothing because the request was sent to the host <code>{shown}</code>, which it does \
             not answer to. That check keeps a web page on another site from reaching a console that has no login.</p>\
             <p>To use this name, start doze-kafka with <code>--console-allow-host {shown}</code>.</p>"
        ),
    )
}

/// 413 for a body that says it is over the limit; a body that does not say is cut at the limit by the extractors.
pub async fn body_limit(req: Request, next: Next) -> Response {
    let length = text(&req, &CONTENT_LENGTH).and_then(|v| v.parse::<usize>().ok());
    if length.is_some_and(|n| n > MAX_BODY) {
        return refusal(
            StatusCode::PAYLOAD_TOO_LARGE,
            "That is too large",
            "<p>The console accepts at most 1 MiB in a request.</p>",
        );
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::escape;

    #[test]
    fn markup_in_a_refused_host_is_written_as_text() {
        assert_eq!(
            escape("<script>alert('x')</script>&\""),
            "&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;&amp;&quot;"
        );
        assert_eq!(escape("127.0.0.1:9093"), "127.0.0.1:9093");
    }
}
