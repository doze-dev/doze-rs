//! Add partitions, Delete records and Delete topic of the React console (slice R1b); the answers as `topic_writes` says them.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::get;
use serde::Serialize;

use super::json;
use super::topic_writes::{done, invalid, kafka_refused, no_topic, query};
use crate::form::{get as field, parse_body, text};
use crate::gather::{AddResult, DeleteResult, RecordsResult};
use crate::model::{
    AddView, DeleteRecordsRefused, DeleteRecordsView, DeleteView, Deleted, Refusal, Span, Teach,
};
use crate::state::Shared;
use crate::write_words;

// ---- Add partitions -----------------------------------------------------------------------------------------------

/// `GET /api/v1/topics/{topic}/add-partitions?count=`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AddForm {
    pub view: AddView,
    pub teach: Teach,
    /// `adds 2: orders-3 and orders-4`, when the count adds any.
    pub adds: Option<String>,
    pub button: String,
}

/// Partitions were added (200).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AddedAnswer {
    pub topic: String,
    pub from: i32,
    pub to: i32,
}

fn add_form_of(view: AddView) -> AddForm {
    AddForm {
        teach: write_words::add_teaching(&view),
        adds: write_words::add_adds(&view),
        button: write_words::add_button(&view),
        view,
    }
}

async fn add_form(
    State(s): State<Shared>,
    Path(topic): Path<String>,
    RawQuery(q): RawQuery,
) -> Response {
    let f = query(q.as_ref());
    match s.source.add_form(&topic, field(&f, "count")).await {
        Some(view) => json(StatusCode::OK, &add_form_of(view)),
        None => no_topic(&topic),
    }
}

async fn add(State(s): State<Shared>, Path(topic): Path<String>, body: Bytes) -> Response {
    let f = parse_body(&body);
    match s.source.add_partitions(&topic, text(&f, "count")).await {
        AddResult::Done { topic, from, to } => done(&AddedAnswer { topic, from, to }),
        AddResult::Refused(v) => match v.refusal.clone() {
            Some(r) => kafka_refused(&r, &add_form_of(*v)),
            None => invalid("Refused.", &add_form_of(*v)),
        },
        AddResult::Invalid(v) => {
            let first = v.error.clone().unwrap_or_default();
            invalid(&first, &add_form_of(*v))
        }
        AddResult::NoTopic => no_topic(&topic),
    }
}

// ---- Delete records -----------------------------------------------------------------------------------------------

/// `GET /api/v1/topics/{topic}/delete-records?partition=&offset=`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RecordsForm {
    pub view: DeleteRecordsView,
    pub teach: Teach,
    /// What the offset typed removes, and who is below it (the preview).
    pub warning: Vec<Span>,
    pub button: String,
    /// The partitions of the topic, for the partition picker.
    pub partitions: Vec<i32>,
}

/// A refused delete-records (the `detail` of a refused outcome).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RecordsRefusedAnswer {
    pub refused: DeleteRecordsRefused,
    pub sentence: String,
}

/// Records were deleted (200): where the log starts now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RecordsDeletedAnswer {
    pub topic: String,
    pub partition: i32,
    pub low_watermark: i64,
}

fn partition_of(f: &[(String, String)]) -> i32 {
    text(f, "partition").trim().parse().unwrap_or(0)
}

async fn records_form_of(s: &Shared, topic: &str, view: DeleteRecordsView) -> RecordsForm {
    let count = s
        .source
        .add_form(topic, None)
        .await
        .map_or(view.partition + 1, |a| a.have);
    RecordsForm {
        teach: write_words::records_teaching(&view),
        warning: write_words::records_warning(&view),
        button: write_words::records_button(&view),
        partitions: (0..count).collect(),
        view,
    }
}

async fn records_form(
    State(s): State<Shared>,
    Path(topic): Path<String>,
    RawQuery(q): RawQuery,
) -> Response {
    let f = query(q.as_ref());
    match s
        .source
        .delete_records_form(&topic, partition_of(&f), field(&f, "offset"))
        .await
    {
        Some(view) => json(StatusCode::OK, &records_form_of(&s, &topic, view).await),
        None => no_topic(&topic),
    }
}

async fn records(State(s): State<Shared>, Path(topic): Path<String>, body: Bytes) -> Response {
    let f = parse_body(&body);
    match s
        .source
        .delete_records(&topic, partition_of(&f), text(&f, "offset"))
        .await
    {
        RecordsResult::Done(d) => done(&RecordsDeletedAnswer {
            topic: d.topic,
            partition: d.partition,
            low_watermark: d.low_watermark,
        }),
        RecordsResult::Refused(r) => {
            let n = crate::format::number(r.log_start);
            let sentence = if r.refusal.code == 44 {
                format!(
                    "Delete records on a topic that only compacts. The partition was not touched: log start is still {n}."
                )
            } else {
                format!(
                    "Delete records was refused. The partition was not touched: log start is still {n}."
                )
            };
            kafka_refused(
                &r.refusal,
                &RecordsRefusedAnswer {
                    sentence,
                    refused: *r.clone(),
                },
            )
        }
        RecordsResult::Invalid(v) => {
            let first = v.error.clone().unwrap_or_default();
            invalid(&first, &records_form_of(&s, &topic, *v).await)
        }
        RecordsResult::NoTopic => no_topic(&topic),
    }
}

// ---- Delete topic -------------------------------------------------------------------------------------------------

/// `GET /api/v1/topics/{topic}/delete`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DeleteForm {
    pub view: DeleteView,
    /// Who uses the topic; empty when nothing does.
    pub in_use: Vec<Span>,
    /// The broker's refusal, when it said no.
    pub refusal: Option<Refusal>,
}

/// A topic that is gone (200).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DeletedAnswer {
    pub deleted: Deleted,
}

fn delete_form_of(view: DeleteView, refusal: Option<Refusal>) -> DeleteForm {
    DeleteForm {
        in_use: write_words::in_use(&view.in_use),
        refusal,
        view,
    }
}

async fn delete_form(State(s): State<Shared>, Path(topic): Path<String>) -> Response {
    match s.source.delete_form(&topic, "").await {
        Some(view) => json(StatusCode::OK, &delete_form_of(view, None)),
        None => no_topic(&topic),
    }
}

async fn delete(State(s): State<Shared>, Path(topic): Path<String>, body: Bytes) -> Response {
    let f = parse_body(&body);
    match s.source.delete_topic(&topic, text(&f, "confirm")).await {
        DeleteResult::Done(d) => done(&DeletedAnswer { deleted: *d }),
        DeleteResult::Refused(v, r) => kafka_refused(&r, &delete_form_of(*v, Some(r.clone()))),
        DeleteResult::Invalid(v) => {
            let first = v.error.clone().unwrap_or_default();
            invalid(&first, &delete_form_of(*v, None))
        }
        DeleteResult::NoTopic => no_topic(&topic),
    }
}

/// The routes of this file under `web`.
pub(super) fn routes(web: &crate::base::Base) -> Router<Shared> {
    Router::new()
        .route(
            &web.url("/api/v1/topics/{topic}/add-partitions"),
            get(add_form).post(add),
        )
        .route(
            &web.url("/api/v1/topics/{topic}/delete-records"),
            get(records_form).post(records),
        )
        .route(
            &web.url("/api/v1/topics/{topic}/delete"),
            get(delete_form).post(delete),
        )
}
