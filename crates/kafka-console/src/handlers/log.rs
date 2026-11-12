//! The Log and Producers pages of a topic, the page of a transaction, and the fragments that follow the data. Every one is a plain
//! GET that changes nothing: the pages are read-only.

use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use kafka_wire::ids::TopicName;
use maud::Markup;

use super::html;
use crate::form::{get, parse};
use crate::pages;
use crate::state::Shared;
use crate::trail::{Place, TopicPage, borrowed};
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
            &borrowed(&Place::NoTopic.steps()),
            &[],
            main,
        )
        .await;
    (StatusCode::NOT_FOUND, html(&doc)).into_response()
}

/// A page of a topic, with the trail of its place and its last step as the title.
async fn shell(s: &Shared, place: Place<'_>, watch: &[&'static str], main: Markup) -> Response {
    let steps = place.steps();
    let title = steps.last().map_or("", |l| l.0.as_str());
    html(
        &s.document(Rail::Resources, title, &borrowed(&steps), watch, main)
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
    let Some(head) = s.source.topic_head(&topic).await else {
        return no_topic(&s, &topic).await;
    };
    let main = docked(
        &s,
        &topic,
        pages::log_page(&v, &head, &s.base, &s.source.listen()),
    )
    .await;
    shell(
        &s,
        Place::TopicPage(&topic, TopicPage::Log),
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
    let Some(head) = s.source.topic_head(&topic).await else {
        return no_topic(&s, &topic).await;
    };
    let main = docked(&s, &topic, pages::producers_page(&v, &head, &s.base)).await;
    shell(
        &s,
        Place::TopicPage(&topic, TopicPage::Producers),
        s.source.watching(crate::stream_log::PRODUCERS_WATCH),
        main,
    )
    .await
}

async fn no_txn(s: &Shared, topic: &str, id: &str) -> Response {
    let shown: String = id.chars().take(80).collect();
    let main = pages::no_txn(topic, &shown, &s.base);
    let r = shell(s, Place::TopicPage(topic, TopicPage::Producers), &[], main).await;
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
    let Some(head) = s.source.topic_head(&topic).await else {
        return no_topic(&s, &topic).await;
    };
    let main = pages::txn_page(&v, &head, &s.base);
    shell(&s, Place::Transaction(&topic, &id), &[], main).await
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
    let Some(head) = s.source.topic_head(&topic).await else {
        return no_topic(&s, &topic).await;
    };
    let main = pages::txn_held_page(&v, &head, &s.base);
    shell(&s, Place::TransactionHeld(&topic, &id), &[], main).await
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
