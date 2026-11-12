//! The Live handlers: thin. Each asks the source for a model, renders it with a page function, and answers.

use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use maud::{Markup, html};

use super::html;
use crate::live_ids::{
    GROUP_WIRE, GROUP_WIRE_IDS, LIVE_IDS, LIVE_WIRE, TOPIC_WIRE, TOPIC_WIRE_IDS,
};
use crate::model::WireQuery;
use crate::pages::live::{
    RequestKind, clients_body, drawer_page, first_page, group_wire_page, live_page, request_body,
    topic_dock,
};
use crate::state::Shared;
use crate::ui::client::HEADER_FRAGMENT;
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

/// The parent of every drawer: the Live page as it is now.
async fn parent(s: &Shared) -> Markup {
    live_page(&s.source.live(&WireQuery::default(), None).await, &s.base)
}

fn fragment_or_page(
    headers: &HeaderMap,
    s: &Shared,
    parent: Markup,
    body: Markup,
    id: &str,
) -> Markup {
    let _ = s;
    if headers.contains_key(HEADER_FRAGMENT) {
        html! { div class="drawer-panel" role="dialog" aria-labelledby=(id) { (body) } }
    } else {
        drawer_page(parent, body, id)
    }
}

/// `/live/{id}`: one request in a drawer over the tape; which drawer, the data chooses.
pub async fn request(State(s): State<Shared>, Path(id): Path<u64>, headers: HeaderMap) -> Response {
    let Some((d, limits)) = s.source.request(id).await else {
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
    let close = s.base.url("/live");
    let kind = RequestKind::of(&d);
    let body = request_body(&d, &limits, &s.base, &s.source.listen(), &close);
    let main = fragment_or_page(&headers, &s, parent(&s).await, body, kind.heading_id());
    if headers.contains_key(HEADER_FRAGMENT) {
        return html(&main);
    }
    html(
        &s.document(
            Rail::Live,
            "Live",
            &[
                ("Live", Some("/live")),
                (&format!("{} v{}", d.row.api, d.row.version), None),
            ],
            &[],
            main,
        )
        .await,
    )
}

/// `/live/clients`: the clients drawer.
pub async fn clients(State(s): State<Shared>, headers: HeaderMap) -> Response {
    let v = s.source.clients().await;
    let body = clients_body(&v, &s.base, &s.base.url("/live"));
    let main = fragment_or_page(&headers, &s, parent(&s).await, body, "live-clients");
    if headers.contains_key(HEADER_FRAGMENT) {
        return html(&main);
    }
    html(
        &s.document(
            Rail::Live,
            "Live",
            &[("Live", Some("/live")), ("Clients", None)],
            &[],
            main,
        )
        .await,
    )
}

/// `/connect/first-request`: what the newest new client asked.
pub async fn first(State(s): State<Shared>) -> Response {
    let Some(v) = s.source.first().await else {
        let main = html! { div class="empty" { h2 { "No client has connected yet" } p { "When one sends its first request it appears here, with the versions it speaks. " a class="lnk" href=(s.base.url("/connect")) { "Snippets for your client" } "." } } };
        return html(
            &s.document(
                Rail::Home,
                "A new client just connected",
                &[("Home", Some("/")), ("First request", None)],
                &[],
                main,
            )
            .await,
        );
    };
    let main = first_page(&v, &s.base);
    html(
        &s.document(
            Rail::Home,
            "First request landed",
            &[
                ("Home", Some("/")),
                ("Connect an app", Some("/connect")),
                ("First request", None),
            ],
            &[],
            main,
        )
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
pub async fn group_wire(State(s): State<Shared>, Path(group): Path<String>) -> Response {
    let (rows, window, now) = s.source.group_wire(&group).await;
    let main = group_wire_page(&group, &rows, &window, now, &s.base);
    let crumbs = [
        ("Resources", Some("/topics")),
        ("Groups", Some("/groups")),
        (group.as_str(), None),
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
            let group = crate::form::text(&f, "group");
            let (rows, window, now) = s.source.group_wire(group).await;
            Some(group_wire_page(group, &rows, &window, now, &s.base))
        }
        _ => None,
    }
}
