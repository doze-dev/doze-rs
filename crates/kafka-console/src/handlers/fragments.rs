//! The fragments a page fetches alone, and the form that asks for a new check.

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};

use super::{etagged, html};
use crate::pages;
use crate::state::Shared;
use crate::ui::client::HEADER_FRAGMENT;

/// A fragment by its id: the parts of Home that change while it is open, and the three renderings of the check.
pub async fn fragment(
    State(s): State<Shared>,
    Path(id): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    if let Some(m) = super::groups_fragment(&s, &id, &query).await {
        return etagged(&headers, &m);
    }
    let Some(markup) = topics_fragment(&s, &id, &query).await else {
        return fragment_of_home(&s, &id, &headers).await;
    };
    etagged(&headers, &markup)
}

/// The fragments of the Topics pages, by id; `None` for an id that is not one of them.
async fn topics_fragment(s: &Shared, id: &str, query: &Option<String>) -> Option<maud::Markup> {
    use crate::pages::topics as t;
    let f = crate::form::parse(query.as_deref().unwrap_or(""));
    let filter = crate::form::get(&f, "q").unwrap_or("").trim().to_owned();
    let internal = crate::form::get(&f, "internal") == Some("1");
    let q = crate::form::encode_query(&[
        ("q", &filter),
        ("internal", if internal { "1" } else { "" }),
    ]);
    match id {
        t::LIST_SENTENCE => Some(t::sentence_fragment(
            &s.source.topics(&filter, internal, None).await,
            &s.base,
            &q,
        )),
        t::LIST_FRAGMENT => Some(t::list_fragment(
            &s.source.topics(&filter, internal, None).await,
            &s.base,
            &q,
        )),
        t::TOPIC_SENTENCE => {
            let name = crate::form::get(&f, "topic").unwrap_or("");
            Some(t::topic_sentence_fragment(
                &s.source.topic_head(name).await?,
                &s.base,
            ))
        }
        t::MESSAGES_FRAGMENT => {
            let name = crate::form::get(&f, "topic").unwrap_or("");
            let (mq, notes) = crate::model::MessageQuery::parse(&f);
            let view = s.source.messages(name, mq, notes).await?;
            Some(t::messages_fragment(&view, &s.base, &view.query.to_query()))
        }
        _ => None,
    }
}

async fn fragment_of_home(s: &Shared, id: &str, headers: &HeaderMap) -> Response {
    let markup = match id {
        // Home's fragment is made of what the broker holds: it never dials (the check is run by the page load and by a person).
        pages::HOME_LIVE => pages::live_fragment(&s.source.home_live().await, &s.base),
        pages::PROBE_HOME => pages::probe_home(&s.source.probe(false).await, &s.base),
        pages::PROBE_CONNECT => pages::probe_connect(&s.source.probe(false).await, &s.base),
        pages::PROBE_ADVERTISED => pages::probe_advertised(&s.source.probe(false).await, &s.base),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    etagged(headers, &markup)
}

/// Runs the advertised-listener check now. With a script the answer is the fragment to swap in; without one it is a
/// redirect back to the page, which shows the new result.
pub async fn check(State(s): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    let fields = crate::form::parse(&String::from_utf8_lossy(&body));
    let (path, target) = match crate::form::get(&fields, "back").unwrap_or("") {
        "home" => ("/", pages::PROBE_HOME),
        "advertised" => ("/connect/advertised", pages::PROBE_ADVERTISED),
        _ => return (StatusCode::BAD_REQUEST, "back must be home or advertised").into_response(),
    };
    let probe = s.source.probe(true).await;
    if headers.contains_key(HEADER_FRAGMENT) {
        let markup = if target == pages::PROBE_HOME {
            pages::probe_home(&probe, &s.base)
        } else {
            pages::probe_advertised(&probe, &s.base)
        };
        return html(&markup);
    }
    Redirect::to(&s.base.url(path)).into_response()
}
