//! The dialogs on a topic: produce, add partitions, delete records, delete topic. Each is a route that answers its
//! body alone to a script (`X-Fragment`) and the whole page, with the dialog in it, to a browser without one; each has a
//! POST that does the write and answers the board's result state.

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;

use super::overlays;
use super::topics::{messages_parent, partitions_parent};
use super::topics_write::page;
use crate::form::{parse, parse_body, text};
use crate::gather::{AddResult, DeleteResult, RecordsResult};
use crate::model::{ProduceInput, ProduceResult};
use crate::pages;
use crate::state::Shared;

/// The most header rows the Produce dialog posts.
const MOST_HEADERS: usize = 8;

/// The page an overlay sits over.
#[derive(Clone, Copy)]
enum Under {
    /// The topic's Messages page (Produce, Delete topic).
    Messages,
    /// The topic's Partitions page (Add partitions, Delete records).
    Partitions,
}

/// An overlay of a topic: alone for a script, over its parent page (with the overlay open on it) for a browser.
async fn dialog(
    s: &Shared,
    status: StatusCode,
    headers: &HeaderMap,
    topic: &str,
    under: Under,
    overlay: Markup,
) -> Response {
    if overlays::wants_fragment(headers) {
        return overlays::fragment(status, &overlay);
    }
    let parent = match under {
        Under::Messages => messages_parent(s, topic, None).await,
        Under::Partitions => partitions_parent(s, topic).await,
    };
    match parent {
        Some(p) => overlays::over_parent(s, status, p, overlay).await,
        None => no_topic(s, topic).await,
    }
}

/// The page that says there is no such topic.
async fn no_topic(s: &Shared, topic: &str) -> Response {
    let main = pages::no_topic(topic, &s.base);
    page(
        s,
        StatusCode::NOT_FOUND,
        "No such topic",
        &[("Topics", Some("/topics")), ("No such topic", None)],
        main,
    )
    .await
}

fn produce_input(fields: &[(String, String)]) -> ProduceInput {
    let headers = (0..MOST_HEADERS)
        .map(|i| {
            (
                text(fields, &format!("hn{i}")).to_owned(),
                text(fields, &format!("hv{i}")).to_owned(),
            )
        })
        .filter(|(n, v)| !n.is_empty() || !v.is_empty())
        .collect();
    ProduceInput {
        partition: text(fields, "partition").to_owned(),
        key: text(fields, "key").to_owned(),
        value: text(fields, "value").to_owned(),
        headers,
        acks: text(fields, "acks").to_owned(),
    }
}

/// `GET /topics/{topic}/produce`
pub async fn produce_form(
    State(s): State<Shared>,
    Path(topic): Path<String>,
    RawQuery(q): RawQuery,
    headers: HeaderMap,
) -> Response {
    let input = produce_input(&parse(q.as_deref().unwrap_or("")));
    let Some(view) = s.source.produce_form(&topic, &input).await else {
        return no_topic(&s, &topic).await;
    };
    dialog(
        &s,
        StatusCode::OK,
        &headers,
        &topic,
        Under::Messages,
        pages::produce_dialog(&view, &s.base),
    )
    .await
}

/// `POST /topics/{topic}/produce`
pub async fn produce(
    State(s): State<Shared>,
    Path(topic): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let input = produce_input(&parse_body(&body));
    match s.source.produce(&topic, &input).await {
        ProduceResult::Sent(p) => {
            let crumbs = [
                ("Topics", Some("/topics")),
                (
                    topic.as_str(),
                    Some(&*format!("/topics/{}", crate::base::segment(&topic))),
                ),
                ("Record sent", None),
            ];
            page(
                &s,
                StatusCode::OK,
                "Record sent",
                &crumbs,
                pages::produced(&p, &s.base),
            )
            .await
        }
        ProduceResult::Refused(r) => {
            let title = if r.refusal.code == 87 && r.view.compacts {
                "Produce refused: no key"
            } else {
                "Produce refused"
            };
            let topic_path = format!("/topics/{}", crate::base::segment(&topic));
            let produce_path = format!("{topic_path}/produce");
            let crumbs = [
                ("Topics", Some("/topics")),
                (topic.as_str(), Some(topic_path.as_str())),
                ("Produce", Some(produce_path.as_str())),
                (title, None),
            ];
            page(
                &s,
                StatusCode::OK,
                title,
                &crumbs,
                pages::produce_refused(&r, &s.base),
            )
            .await
        }
        ProduceResult::Invalid(view) => {
            dialog(
                &s,
                StatusCode::UNPROCESSABLE_ENTITY,
                &headers,
                &topic,
                Under::Messages,
                pages::produce_dialog(&view, &s.base),
            )
            .await
        }
        ProduceResult::NoTopic => no_topic(&s, &topic).await,
    }
}

/// `GET /topics/{topic}/partitions/add`
pub async fn partitions_add_form(
    State(s): State<Shared>,
    Path(topic): Path<String>,
    RawQuery(q): RawQuery,
    headers: HeaderMap,
) -> Response {
    let fields = parse(q.as_deref().unwrap_or(""));
    let Some(view) = s
        .source
        .add_form(&topic, crate::form::get(&fields, "count"))
        .await
    else {
        return no_topic(&s, &topic).await;
    };
    dialog(
        &s,
        StatusCode::OK,
        &headers,
        &topic,
        Under::Partitions,
        pages::add_partitions_dialog(&view, &s.base),
    )
    .await
}

/// `POST /topics/{topic}/partitions/add`
pub async fn partitions_add(
    State(s): State<Shared>,
    Path(topic): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let fields = parse_body(&body);
    match s
        .source
        .add_partitions(&topic, text(&fields, "count"))
        .await
    {
        AddResult::Done { topic, .. } => Redirect::to(&s.base.url(&format!(
            "/topics/{}/partitions",
            crate::base::segment(&topic)
        )))
        .into_response(),
        AddResult::Refused(v) => {
            dialog(
                &s,
                StatusCode::OK,
                &headers,
                &topic,
                Under::Partitions,
                pages::add_partitions_dialog(&v, &s.base),
            )
            .await
        }
        AddResult::Invalid(v) => {
            dialog(
                &s,
                StatusCode::UNPROCESSABLE_ENTITY,
                &headers,
                &topic,
                Under::Partitions,
                pages::add_partitions_dialog(&v, &s.base),
            )
            .await
        }
        AddResult::NoTopic => no_topic(&s, &topic).await,
    }
}

fn partition_of(fields: &[(String, String)]) -> i32 {
    text(fields, "partition").trim().parse().unwrap_or(0)
}

/// `GET /topics/{topic}/delete-records?partition=0`
pub async fn delete_records_form(
    State(s): State<Shared>,
    Path(topic): Path<String>,
    RawQuery(q): RawQuery,
    headers: HeaderMap,
) -> Response {
    let fields = parse(q.as_deref().unwrap_or(""));
    let Some(view) = s
        .source
        .delete_records_form(
            &topic,
            partition_of(&fields),
            crate::form::get(&fields, "offset"),
        )
        .await
    else {
        return no_topic(&s, &topic).await;
    };
    dialog(
        &s,
        StatusCode::OK,
        &headers,
        &topic,
        Under::Partitions,
        pages::delete_records_dialog(&view, &s.base),
    )
    .await
}

/// `POST /topics/{topic}/delete-records`
pub async fn delete_records(
    State(s): State<Shared>,
    Path(topic): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let fields = parse_body(&body);
    match s
        .source
        .delete_records(&topic, partition_of(&fields), text(&fields, "offset"))
        .await
    {
        RecordsResult::Done(d) => Redirect::to(&s.base.url(&format!(
            "/topics/{}/partitions",
            crate::base::segment(&d.topic)
        )))
        .into_response(),
        RecordsResult::Refused(r) => {
            let topic_path = format!("/topics/{}", crate::base::segment(&topic));
            let parts = format!("{topic_path}/partitions");
            let crumbs = [
                ("Topics", Some("/topics")),
                (topic.as_str(), Some(topic_path.as_str())),
                ("Partitions", Some(parts.as_str())),
                ("Delete records refused", None),
            ];
            page(
                &s,
                StatusCode::OK,
                "Delete records refused",
                &crumbs,
                pages::delete_records_refused(&r, &s.base),
            )
            .await
        }
        RecordsResult::Invalid(v) => {
            dialog(
                &s,
                StatusCode::UNPROCESSABLE_ENTITY,
                &headers,
                &topic,
                Under::Partitions,
                pages::delete_records_dialog(&v, &s.base),
            )
            .await
        }
        RecordsResult::NoTopic => no_topic(&s, &topic).await,
    }
}

/// `GET /topics/{topic}/delete`
pub async fn delete_form(
    State(s): State<Shared>,
    Path(topic): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(view) = s.source.delete_form(&topic, "").await else {
        return no_topic(&s, &topic).await;
    };
    dialog(
        &s,
        StatusCode::OK,
        &headers,
        &topic,
        Under::Messages,
        pages::delete_dialog(&view, None, &s.base),
    )
    .await
}

/// `POST /topics/{topic}/delete`
pub async fn delete(
    State(s): State<Shared>,
    Path(topic): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let fields = parse_body(&body);
    match s
        .source
        .delete_topic(&topic, text(&fields, "confirm"))
        .await
    {
        DeleteResult::Done(d) => {
            page(
                &s,
                StatusCode::OK,
                "Topic deleted",
                &[("Topics", Some("/topics")), ("Topic deleted", None)],
                pages::deleted_page(&d, &s.base),
            )
            .await
        }
        DeleteResult::Refused(v, r) => {
            dialog(
                &s,
                StatusCode::OK,
                &headers,
                &topic,
                Under::Messages,
                pages::delete_dialog(&v, Some(&r), &s.base),
            )
            .await
        }
        DeleteResult::Invalid(v) => {
            dialog(
                &s,
                StatusCode::UNPROCESSABLE_ENTITY,
                &headers,
                &topic,
                Under::Messages,
                pages::delete_dialog(&v, None, &s.base),
            )
            .await
        }
        DeleteResult::NoTopic => no_topic(&s, &topic).await,
    }
}
