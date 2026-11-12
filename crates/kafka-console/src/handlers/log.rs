//! The Log and Producers pages of a topic, the page of a transaction, and the fragments that follow the data. Every one is a plain
//! GET that changes nothing: the pages are read-only.

use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use kafka_wire::ids::TopicName;
use maud::Markup;

use super::html;
use crate::base::segment;
use crate::form::{get, parse};
use crate::pages;
use crate::state::Shared;
use crate::ui::shell::Rail;

/// The most producers a page may be asked to open.
const MOST_OPEN: usize = 16;

fn valid(name: &str) -> bool {
    TopicName::parse(name).is_ok()
}

/// `?partition=`: a non-negative whole number, else the first.
fn partition_of(query: &Option<String>) -> i32 {
    let f = parse(query.as_deref().unwrap_or(""));
    get(&f, "partition")
        .and_then(|p| p.parse::<i32>().ok())
        .filter(|p| *p >= 0)
        .unwrap_or(0)
}

fn opened_of(query: &Option<String>) -> Vec<i64> {
    let f = parse(query.as_deref().unwrap_or(""));
    get(&f, "open")
        .unwrap_or("")
        .split(',')
        .filter_map(|p| p.parse::<i64>().ok())
        .take(MOST_OPEN)
        .collect()
}

async fn no_topic(s: &Shared, topic: &str) -> Response {
    let shown: String = topic.chars().take(80).collect();
    let main = pages::no_topic(&shown, &s.base);
    let doc = s
        .document(
            Rail::Resources,
            "No such topic",
            &[
                ("Resources", Some("/topics")),
                ("Topics", Some("/topics")),
                ("No such topic", None),
            ],
            &[],
            main,
        )
        .await;
    (StatusCode::NOT_FOUND, html(&doc)).into_response()
}

type Steps<'a> = Vec<(&'a str, Option<&'a str>)>;

/// A page of a topic: `Resources > Topics > topic > tail...`.
async fn shell(
    s: &Shared,
    topic: &str,
    tail: &[(&str, Option<String>)],
    watch: &[&'static str],
    main: Markup,
) -> Response {
    let t = format!("/topics/{}", segment(topic));
    let mut steps: Steps = vec![
        ("Resources", Some("/topics")),
        ("Topics", Some("/topics")),
        (topic, Some(&t)),
    ];
    for (label, path) in tail {
        steps.push((label, path.as_deref()));
    }
    let title = tail.last().map_or(topic, |l| l.0);
    html(
        &s.document(Rail::Resources, title, &steps, watch, main)
            .await,
    )
    .into_response()
}

/// The page with the topic's "On the wire" dock under it, as the Messages page has it: the last requests that touched the topic.
async fn docked(s: &Shared, topic: &str, main: Markup) -> Markup {
    let (rows, window, now) = s.source.topic_wire(topic).await;
    maud::html! { (main) (crate::pages::live::topic_dock(topic, &rows, &window, now, &s.base, false)) }
}

/// `GET /topics/{topic}/log`
pub async fn log(
    State(s): State<Shared>,
    Path(topic): Path<String>,
    RawQuery(q): RawQuery,
) -> Response {
    if !valid(&topic) {
        return no_topic(&s, &topic).await;
    }
    let Some(v) = s.source.log(&topic, partition_of(&q)).await else {
        return no_topic(&s, &topic).await;
    };
    let main = docked(&s, &topic, pages::log_page(&v, &s.base, &s.source.listen())).await;
    shell(
        &s,
        &topic,
        &[("Log", None)],
        s.source.watching(crate::stream_log::LOG_WATCH),
        main,
    )
    .await
}

/// `GET /topics/{topic}/producers`
pub async fn producers(
    State(s): State<Shared>,
    Path(topic): Path<String>,
    RawQuery(q): RawQuery,
) -> Response {
    if !valid(&topic) {
        return no_topic(&s, &topic).await;
    }
    let Some(v) = s
        .source
        .producers(&topic, partition_of(&q), opened_of(&q))
        .await
    else {
        return no_topic(&s, &topic).await;
    };
    let main = docked(&s, &topic, pages::producers_page(&v, &s.base)).await;
    shell(
        &s,
        &topic,
        &[("Producers", None)],
        s.source.watching(crate::stream_log::PRODUCERS_WATCH),
        main,
    )
    .await
}

async fn no_txn(s: &Shared, topic: &str, id: &str) -> Response {
    let shown: String = id.chars().take(80).collect();
    let main = pages::no_txn(topic, &shown, &s.base);
    let p = format!("/topics/{}/producers", segment(topic));
    let r = shell(
        s,
        topic,
        &[("Producers", Some(p)), ("No such transaction", None)],
        &[],
        main,
    )
    .await;
    let (mut parts, body) = r.into_parts();
    parts.status = StatusCode::NOT_FOUND;
    Response::from_parts(parts, body)
}

/// `GET /topics/{topic}/transactions/{id}`
pub async fn txn(State(s): State<Shared>, Path((topic, id)): Path<(String, String)>) -> Response {
    if !valid(&topic) {
        return no_topic(&s, &topic).await;
    }
    let Some(v) = s.source.txn(&topic, &id).await else {
        return no_txn(&s, &topic, &id).await;
    };
    let main = pages::txn_page(&v, &s.base);
    let p = format!("/topics/{}/producers", segment(&topic));
    shell(
        &s,
        &topic,
        &[("Producers", Some(p)), (&id, None)],
        &[],
        main,
    )
    .await
}

/// `GET /topics/{topic}/transactions/{id}/held`
pub async fn txn_held(
    State(s): State<Shared>,
    Path((topic, id)): Path<(String, String)>,
) -> Response {
    if !valid(&topic) {
        return no_topic(&s, &topic).await;
    }
    let Some(v) = s.source.txn(&topic, &id).await else {
        return no_txn(&s, &topic, &id).await;
    };
    let main = pages::txn_held_page(&v, &s.base);
    let p = format!("/topics/{}/producers", segment(&topic));
    let t = format!("/topics/{}/transactions/{}", segment(&topic), segment(&id));
    shell(
        &s,
        &topic,
        &[
            ("Producers", Some(p)),
            (&id, Some(t)),
            ("What it holds back", None),
        ],
        &[],
        main,
    )
    .await
}

/// The fragments of these pages, by id; `None` for an id that is not one of them.
pub async fn log_fragment(s: &Shared, id: &str, query: &Option<String>) -> Option<Markup> {
    let f = parse(query.as_deref().unwrap_or(""));
    let topic = get(&f, "topic")?;
    let partition = get(&f, "partition")
        .and_then(|p| p.parse::<i32>().ok())
        .filter(|p| *p >= 0)
        .unwrap_or(0);
    match id {
        pages::LOG_SENTENCE | pages::LOG_RULER | pages::LOG_CARDS => {
            let v = s.source.log(topic, partition).await?;
            Some(match id {
                pages::LOG_SENTENCE => pages::log_sentence_fragment(&v, &s.base),
                pages::LOG_RULER => pages::log_ruler_fragment(&v, &s.base),
                _ => pages::log_cards_fragment(&v, &s.base),
            })
        }
        pages::PRODUCERS_SENTENCE | pages::PRODUCERS_TABLE => {
            let v = s
                .source
                .producers(topic, partition, opened_of(query))
                .await?;
            Some(if id == pages::PRODUCERS_SENTENCE {
                pages::producers_sentence_fragment(&v, &s.base)
            } else {
                pages::producers_table_fragment(&v, &s.base)
            })
        }
        _ => None,
    }
}
