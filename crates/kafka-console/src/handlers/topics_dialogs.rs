//! The dialogs on a topic: produce, add partitions, delete records, delete topic. Each is a route that answers its
//! body alone to a script (`X-Fragment`) and the whole page, with the dialog in it, to a browser without one; each has a
//! POST that does the write and answers the board's result state.

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;

use super::html;
use super::topics_write::page;
use crate::form::{parse, parse_body, text};
use crate::gather::{AddResult, DeleteResult, RecordsResult};
use crate::model::{ProduceInput, ProduceResult};
use crate::pages;
use crate::state::Shared;
use crate::ui::client::HEADER_FRAGMENT;
use crate::ui::shell::Rail;

/// The most header rows the Produce dialog posts.
const MOST_HEADERS: usize = 8;

fn asked_for_a_fragment(h: &HeaderMap) -> bool {
    h.contains_key(HEADER_FRAGMENT)
}

/// The dialog on a page of its own (or alone, for a script), under its breadcrumb.
/// Where a dialog sits: its title, the topic, and the pages between the topic and it (label and path under the topic).
struct At<'a> {
    title: &'a str,
    topic: &'a str,
    trail: &'a [(&'a str, &'a str)],
}

async fn dialog(
    s: &Shared,
    status: StatusCode,
    headers: &HeaderMap,
    at: At<'_>,
    make: impl Fn(bool) -> Markup,
) -> Response {
    let At {
        title,
        topic,
        trail,
    } = at;
    if asked_for_a_fragment(headers) {
        return (status, html(&make(false))).into_response();
    }
    let topic_path = format!("/topics/{}", crate::base::segment(topic));
    let mut crumbs: Vec<(&str, Option<&str>)> = vec![
        ("Topics", Some("/topics")),
        (topic, Some(topic_path.as_str())),
    ];
    let trail_paths: Vec<String> = trail
        .iter()
        .map(|(_, tail)| format!("{topic_path}{tail}"))
        .collect();
    for ((label, _), path) in trail.iter().zip(&trail_paths) {
        crumbs.push((label, Some(path.as_str())));
    }
    crumbs.push((title, None));
    let doc = s
        .document(Rail::Resources, title, &crumbs, &[], make(true))
        .await;
    (status, html(&doc)).into_response()
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
        At {
            title: "Produce",
            topic: &topic,
            trail: &[],
        },
        |inline| pages::produce_dialog(&view, &s.base, inline),
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
                At {
                    title: "Produce",
                    topic: &topic,
                    trail: &[],
                },
                |inline| pages::produce_dialog(&view, &s.base, inline),
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
        At {
            title: "Add partitions",
            topic: &topic,
            trail: &[("Partitions", "/partitions")],
        },
        |inline| pages::add_partitions_dialog(&view, &s.base, inline),
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
                At {
                    title: "Add partitions refused",
                    topic: &topic,
                    trail: &[("Partitions", "/partitions")],
                },
                |inline| pages::add_partitions_dialog(&v, &s.base, inline),
            )
            .await
        }
        AddResult::Invalid(v) => {
            dialog(
                &s,
                StatusCode::UNPROCESSABLE_ENTITY,
                &headers,
                At {
                    title: "Add partitions",
                    topic: &topic,
                    trail: &[("Partitions", "/partitions")],
                },
                |inline| pages::add_partitions_dialog(&v, &s.base, inline),
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
        At {
            title: "Delete records",
            topic: &topic,
            trail: &[("Partitions", "/partitions")],
        },
        |inline| pages::delete_records_dialog(&view, &s.base, inline),
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
                At {
                    title: "Delete records",
                    topic: &topic,
                    trail: &[("Partitions", "/partitions")],
                },
                |inline| pages::delete_records_dialog(&v, &s.base, inline),
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
        At {
            title: "Delete topic",
            topic: &topic,
            trail: &[],
        },
        |inline| pages::delete_dialog(&view, None, &s.base, inline),
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
                At {
                    title: "Delete topic",
                    topic: &topic,
                    trail: &[],
                },
                |inline| pages::delete_dialog(&v, Some(&r), &s.base, inline),
            )
            .await
        }
        DeleteResult::Invalid(v) => {
            dialog(
                &s,
                StatusCode::UNPROCESSABLE_ENTITY,
                &headers,
                At {
                    title: "Delete topic",
                    topic: &topic,
                    trail: &[],
                },
                |inline| pages::delete_dialog(&v, None, &s.base, inline),
            )
            .await
        }
        DeleteResult::NoTopic => no_topic(&s, &topic).await,
    }
}
