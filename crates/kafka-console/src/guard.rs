//! The two refusals in front of every route: a `Host` the guard does not allow, and a change with no same-origin proof;
//! and the limit on what a request may bring.

use axum::extract::{Request, State};
use axum::http::header::{CONTENT_LENGTH, HOST, ORIGIN};
use axum::http::{HeaderName, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use kafka_auth::hostguard;
use maud::{DOCTYPE, Markup, html};

use crate::origin;
use crate::state::Shared;

/// The most a request body may be: a form is a few hundred bytes, and a 1 MiB body is refused unread.
pub const MAX_BODY: usize = 1 << 20;

const FETCH_SITE: HeaderName = HeaderName::from_static("sec-fetch-site");

fn text<'a>(req: &'a Request, name: &HeaderName) -> Option<&'a str> {
    req.headers().get(name).and_then(|v| v.to_str().ok())
}

/// A small page for a refusal: no shell (the guard answers before anything else is built), nothing from the request
/// except what is escaped.
fn refusal(status: StatusCode, title: &str, body: Markup) -> Response {
    let page = html! {
        (DOCTYPE)
        html lang="en" {
            head { meta charset="utf-8"; title { (title) " · doze-kafka" } }
            body style="font-family:system-ui,sans-serif;max-width:40rem;margin:4rem auto;padding:0 1rem;line-height:1.5" {
                h1 { (title) }
                (body)
            }
        }
    };
    (
        status,
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        page.into_string(),
    )
        .into_response()
}

/// 403 for a `Host` the guard does not allow, naming the one it saw and the flag that allows another.
pub async fn host_guard(State(s): State<Shared>, req: Request, next: Next) -> Response {
    let host = text(&req, &HOST).unwrap_or("");
    if hostguard::allowed(host, &s.allow_hosts) {
        return next.run(req).await;
    }
    let shown: String = host.chars().take(120).collect();
    refusal(
        StatusCode::FORBIDDEN,
        "This Host is not allowed",
        html! {
            p { "The console answered nothing because the request was sent to the host " code { (shown) }
                ", which it does not answer to. That check keeps a web page on another site from reaching a console that has no login." }
            p { "To use this name, start doze-kafka with " code { "--console-allow-host " (shown) } "." }
        },
    )
}

/// 403 for a request that changes something and does not prove it came from the console's own pages.
pub async fn same_origin(req: Request, next: Next) -> Response {
    let verdict = origin::check(
        req.method().as_str(),
        text(&req, &ORIGIN),
        text(&req, &FETCH_SITE),
        text(&req, &HOST),
    );
    match verdict {
        Ok(()) => next.run(req).await,
        Err(why) => refusal(
            StatusCode::FORBIDDEN,
            "This request was refused",
            html! { p { (why.words()) } },
        ),
    }
}

/// 413 for a body that says it is over the limit; a body that does not say is cut at the limit by the extractors.
pub async fn body_limit(req: Request, next: Next) -> Response {
    let length = text(&req, &CONTENT_LENGTH).and_then(|v| v.parse::<usize>().ok());
    if length.is_some_and(|n| n > MAX_BODY) {
        return refusal(
            StatusCode::PAYLOAD_TOO_LARGE,
            "That is too large",
            html! { p { "The console accepts at most 1 MiB in a request." } },
        );
    }
    next.run(req).await
}
