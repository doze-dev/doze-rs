//! The Data page (docs/port/data-control.md): what is using the disk and why, and Empty topic, Empty all topics and Erase
//! everything. The writes answer as the topic writes do (`topic_writes`): a 200 with `outcome` `done`, `refused` (Kafka's
//! error name and the broker's sentence, `detail` the refusal to draw) or `invalid` (the phrase typed was not the one asked).

use axum::Router;
use axum::body::Bytes;
use axum::extract::{RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use kafka_broker::DataRefusal;
use serde::Serialize;

use super::json;
use super::topic_writes::{done, invalid, kafka_refused, no_topic, query};
use crate::data_words::{self, EMPTY_ALL_PHRASE, ERASE_PHRASE};
use crate::form::{get as field, parse_body, text};
use crate::model::{DataView, EmptyTopicView, Refusal};
use crate::state::Shared;

/// The words of the dialogs that do not depend on the data: what each action removes and keeps, and the phrases typed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DataWords {
    pub empty_all_phrase: String,
    pub erase_phrase: String,
    /// What Empty all topics keeps, line by line.
    pub empty_all_keeps: Vec<String>,
    /// What Erase everything removes, and what it keeps.
    pub erase_removes: Vec<String>,
    pub erase_keeps: Vec<String>,
}

/// `GET /api/v1/data`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DataResponse {
    pub view: DataView,
    pub words: DataWords,
}

/// `GET /api/v1/data/empty?topic=`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct EmptyForm {
    pub view: EmptyTopicView,
    pub button: String,
}

/// A topic was emptied (200).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct EmptiedAnswer {
    pub topic: String,
    pub partitions: usize,
    pub records: i64,
    pub freed_bytes: u64,
    pub sentence: String,
}

/// Every topic was emptied (200).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct EmptiedAllAnswer {
    pub topics: usize,
    pub records: i64,
    pub freed_bytes: u64,
    pub sentence: String,
}

/// Everything was erased (200).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ErasedAnswer {
    pub topics: usize,
    pub groups: usize,
    pub groups_kept: Vec<String>,
    pub transactional_ids: usize,
    /// Subjects the registry removed; `None` when there is no registry.
    pub subjects: Option<usize>,
    pub freed_bytes: u64,
    pub sentence: String,
    /// The broker was erased, and the registry refused: its words.
    pub registry_error: Option<String>,
}

fn words() -> DataWords {
    DataWords {
        empty_all_phrase: EMPTY_ALL_PHRASE.to_owned(),
        erase_phrase: ERASE_PHRASE.to_owned(),
        empty_all_keeps: data_words::empty_all_keeps(),
        erase_removes: data_words::erase_removes(),
        erase_keeps: data_words::erase_keeps(),
    }
}

/// A refusal of the broker's, as the page draws it.
fn refused(r: &DataRefusal) -> Response {
    let refusal: Refusal = crate::refusal::refusal(r.error().code(), Some(&r.sentence()));
    kafka_refused(&refusal, &refusal)
}

async fn page(State(s): State<Shared>) -> Response {
    match s.source.data().await {
        Some(view) => json(
            StatusCode::OK,
            &DataResponse {
                view,
                words: words(),
            },
        ),
        None => super::topics::missing(
            "CLUSTER_AUTHORIZATION_FAILED",
            "The console may not describe the cluster, so it cannot say what the data directory holds.",
        ),
    }
}

async fn empty_form(State(s): State<Shared>, RawQuery(q): RawQuery) -> Response {
    let f = query(q.as_ref());
    let topic = field(&f, "topic").unwrap_or("");
    match s.source.empty_form(topic).await {
        Some(view) => json(
            StatusCode::OK,
            &EmptyForm {
                button: data_words::empty_button(&view),
                view,
            },
        ),
        None => no_topic(topic),
    }
}

async fn empty(State(s): State<Shared>, body: Bytes) -> Response {
    let f = parse_body(&body);
    let topic = text(&f, "topic");
    match s.source.empty_topic(topic).await {
        Ok(e) => done(&EmptiedAnswer {
            sentence: data_words::emptied(&e),
            topic: e.topic,
            partitions: e.partitions,
            records: e.offsets,
            freed_bytes: e.freed_bytes,
        }),
        Err(DataRefusal::NoTopic { topic }) => no_topic(&topic),
        Err(r) => refused(&r),
    }
}

/// The phrase typed was not the one asked: the dialog says so under the field.
fn mistyped(phrase: &str) -> Response {
    let message = format!("Type {phrase} to confirm.");
    invalid(&message, &message)
}

async fn empty_all(State(s): State<Shared>, body: Bytes) -> Response {
    let f = parse_body(&body);
    if text(&f, "confirm").trim() != EMPTY_ALL_PHRASE {
        return mistyped(EMPTY_ALL_PHRASE);
    }
    match s.source.empty_all().await {
        Ok(e) => done(&EmptiedAllAnswer {
            sentence: data_words::emptied_all(&e),
            topics: e.topics.len(),
            records: e.offsets(),
            freed_bytes: e.freed_bytes(),
        }),
        Err(r) => refused(&r),
    }
}

async fn erase(State(s): State<Shared>, body: Bytes) -> Response {
    let f = parse_body(&body);
    if text(&f, "confirm").trim() != ERASE_PHRASE {
        return mistyped(ERASE_PHRASE);
    }
    match s.source.erase().await {
        Ok(e) => {
            let subjects = e.subjects.as_ref().and_then(|r| r.as_ref().ok().copied());
            done(&ErasedAnswer {
                sentence: data_words::erased(&e.broker, subjects),
                topics: e.broker.topics,
                groups: e.broker.groups,
                groups_kept: e.broker.groups_kept.clone(),
                transactional_ids: e.broker.transactional_ids,
                subjects,
                freed_bytes: e.broker.freed_bytes,
                registry_error: e.subjects.and_then(Result::err),
            })
        }
        Err(r) => refused(&r),
    }
}

/// The routes of this file under `web`.
pub(super) fn routes(web: &crate::base::Base) -> Router<Shared> {
    Router::new()
        .route(&web.url("/api/v1/data"), get(page))
        .route(&web.url("/api/v1/data/empty"), get(empty_form).post(empty))
        .route(&web.url("/api/v1/data/empty-all"), post(empty_all))
        .route(&web.url("/api/v1/data/erase"), post(erase))
}

/// The types of this file and of the Data page's view-models.
#[cfg(test)]
pub(super) fn types(cfg: &ts_rs::Config, out: &mut std::collections::BTreeMap<String, String>) {
    use super::types::one;
    use crate::model::{AlarmTone, DataKind, DataKindRow, DataTopicRow, DiskAlarm, VolumeView};
    one::<DataResponse>(cfg, out);
    one::<DataWords>(cfg, out);
    one::<DataView>(cfg, out);
    one::<DataKind>(cfg, out);
    one::<DataKindRow>(cfg, out);
    one::<DataTopicRow>(cfg, out);
    one::<VolumeView>(cfg, out);
    one::<AlarmTone>(cfg, out);
    one::<DiskAlarm>(cfg, out);
    one::<EmptyTopicView>(cfg, out);
    one::<EmptyForm>(cfg, out);
    one::<EmptiedAnswer>(cfg, out);
    one::<EmptiedAllAnswer>(cfg, out);
    one::<ErasedAnswer>(cfg, out);
}

#[cfg(test)]
mod tests;
