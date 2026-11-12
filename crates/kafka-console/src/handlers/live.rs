//! The Live handlers: thin. Each asks the source for a model, renders it with a page function, and answers.

use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use maud::{Markup, html};

use super::html;
use super::overlays::{self, Parent};
use crate::live_ids::{
    GROUP_WIRE, GROUP_WIRE_IDS, LIVE_IDS, LIVE_WIRE, TOPIC_WIRE, TOPIC_WIRE_IDS,
};
use crate::model::WireQuery;
use crate::pages::live::{
    clients_drawer, first_page, group_wire_page, live_page, request_drawer, topic_dock,
};
use crate::state::Shared;
use crate::ui::shell::Rail;

const LIVE: (&str, Option<&str>) = ("Live", None);

fn fields(query: &Option<String>) -> Vec<(String, String)> {
    crate::form::parse(query.as_deref().unwrap_or(""))
}

/// `/live`: the tape, in the state its data calls for (and `?until=` for the page as it was at a pause).
pub async fn live(State(s): State<Shared>, RawQuery(query): RawQuery) -> Response {
    let f = fields(&query);
    let q = WireQuery::parse(&f);
    let view = s.source.live(&q, crate::form::get(&f, "state")).await;
    let main = live_page(&view, &s.base);
    html(
        &s.document(
            Rail::Live,
            "Live",
            &[LIVE],
            s.source.watching(LIVE_IDS),
            main,
        )
        .await,
    )
}

/// The Live page as the parent of a drawer: the same page `live` renders for the same filters, with its title, breadcrumb and
/// stream, so that it keeps running behind the drawer (an open drawer holds the tape: the script keeps what comes aside).
async fn live_parent(s: &Shared, q: &WireQuery) -> Parent {
    let main = live_page(&s.source.live(q, None).await, &s.base);
    Parent {
        rail: Rail::Live,
        title: "Live".to_owned(),
        crumbs: vec![("Live".to_owned(), None)],
        watch: s.source.watching(LIVE_IDS),
        main,
    }
}

/// The address a drawer closes to: the Live page with the filters the drawer was opened from.
fn live_url(s: &Shared, q: &WireQuery) -> String {
    format!("{}{}", s.base.url("/live"), q.to_query())
}

/// `/live/{id}`: one request in a drawer over the tape (which drawer, the data chooses), over the Live page with the filters in
/// the address; closing returns to it.
pub async fn request(
    State(s): State<Shared>,
    Path(id): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    // An id is a number; anything else is a request that is not on the tape.
    let found = match id.parse::<u64>() {
        Ok(id) => s.source.request(id).await,
        Err(_) => None,
    };
    let Some((d, limits)) = found else {
        let main = html! { div class="empty" { h2 { "That request is no longer on the wire" } p { "The recorder keeps a bounded tape, and this row has fallen off it. " a class="lnk" href=(s.base.url("/live")) { "Back to Live" } "." } } };
        let page = s
            .document(
                Rail::Live,
                "Live",
                &[("Live", Some("/live")), ("Request", None)],
                &[],
                main,
            )
            .await;
        return (StatusCode::NOT_FOUND, html(&page)).into_response();
    };
    let q = WireQuery::parse(&fields(&query));
    let drawer = request_drawer(&d, &limits, &s.base, &s.source.listen(), &live_url(&s, &q));
    if overlays::wants_fragment(&headers) {
        return overlays::fragment(StatusCode::OK, &drawer);
    }
    overlays::over_parent(&s, StatusCode::OK, live_parent(&s, &q).await, drawer).await
}

/// `/live/clients`: the clients drawer over the Live page.
pub async fn clients(
    State(s): State<Shared>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let v = s.source.clients().await;
    let q = WireQuery::parse(&fields(&query));
    let drawer = clients_drawer(&v, &s.base, &live_url(&s, &q));
    if overlays::wants_fragment(&headers) {
        return overlays::fragment(StatusCode::OK, &drawer);
    }
    overlays::over_parent(&s, StatusCode::OK, live_parent(&s, &q).await, drawer).await
}

/// `/connect/first-request`: what the newest new client asked.
pub async fn first(State(s): State<Shared>) -> Response {
    let Some(v) = s.source.first().await else {
        let main = html! { div class="empty" { h2 { "No client has connected yet" } p { "When one sends its first request it appears here, with the versions it speaks. " a class="lnk" href=(s.base.url("/connect")) { "Snippets for your client" } "." } } };
        return html(
            &s.document(
                Rail::Home,
                "A new client just connected",
                &[("Home", None)],
                &[],
                main,
            )
            .await,
        );
    };
    let main = first_page(&v, &s.base);
    html(
        &s.document(Rail::Home, "Home", &[("Home", None)], &[], main)
            .await,
    )
}

/// `/topics/{topic}/wire`: the topic's page with its wire dock opened up.
pub async fn topic_wire(State(s): State<Shared>, Path(topic): Path<String>) -> Response {
    let Some(head) = s.source.topic_head(&topic).await else {
        let main = crate::pages::topics::unknown(&topic, &s.base);
        let shown: String = topic.chars().take(60).collect();
        let page = s
            .document(
                Rail::Resources,
                "No such topic",
                &[
                    ("Resources", Some("/topics")),
                    ("Topics", Some("/topics")),
                    (&shown, None),
                ],
                &[],
                main,
            )
            .await;
        return (StatusCode::NOT_FOUND, html(&page)).into_response();
    };
    let (rows, window, now) = s.source.topic_wire(&topic).await;
    let dock = topic_dock(&topic, &rows, &window, now, &s.base, true);
    let main = crate::pages::topics::topic_wire_page(&head, dock, &s.base);
    let crumbs = [
        ("Resources", Some("/topics")),
        ("Topics", Some("/topics")),
        (
            topic.as_str(),
            Some(&*format!("/topics/{}", crate::base::segment(&topic))),
        ),
        ("On the wire", None),
    ];
    html(
        &s.document(
            Rail::Resources,
            &format!("{topic} on the wire"),
            &crumbs,
            s.source.watching(TOPIC_WIRE_IDS),
            main,
        )
        .await,
    )
}

/// `/groups/{group}/wire`: every request that touched a group. The group pages are slice 9c's; this is its body, in the shell.
pub async fn group_wire(
    State(s): State<Shared>,
    Path(group): Path<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let q = WireQuery {
        group: group.clone(),
        ..WireQuery::parse(&fields(&query))
    };
    let main = group_wire_page(&s.source.group_wire(&q).await, &s.base);
    let g = format!("/groups/{}", crate::base::segment(&group));
    let crumbs = [
        ("Resources", Some("/topics")),
        ("Groups", Some("/groups")),
        (group.as_str(), Some(g.as_str())),
        ("Wire", None),
    ];
    html(
        &s.document(
            Rail::Resources,
            "Wire",
            &crumbs,
            s.source.watching(GROUP_WIRE_IDS),
            main,
        )
        .await,
    )
}

/// The fragments of the Live pages, by id; `None` for an id that is not one of them.
pub(crate) async fn fragment(s: &Shared, id: &str, query: &Option<String>) -> Option<Markup> {
    let f = fields(query);
    match id {
        LIVE_WIRE => {
            let q = WireQuery::parse(&f);
            Some(live_page(
                &s.source.live(&q, crate::form::get(&f, "state")).await,
                &s.base,
            ))
        }
        TOPIC_WIRE => {
            let topic = crate::form::text(&f, "topic");
            let (rows, window, now) = s.source.topic_wire(topic).await;
            Some(topic_dock(topic, &rows, &window, now, &s.base, false))
        }
        GROUP_WIRE => {
            let q = WireQuery::parse(&f);
            Some(group_wire_page(&s.source.group_wire(&q).await, &s.base))
        }
        _ => None,
    }
}
