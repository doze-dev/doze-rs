//! The JSON API of the Groups pages (slice R2): the list, one group with its lag, its story, the requests that touched it,
//! and the three writes (reset offsets, delete the group, delete offsets) with what their dialogs show before anything is
//! sent. The data is the classic console's own (`gather::groups`, `gather::group`, `gather::story`; the requests that touched a group are the Live slice's `/api/v1/live/group-wire`);
//! a write goes through the same `Source` call as the classic form and answers Kafka's refusal in Kafka's words.
//!
//! A group's id is hostile input: it reaches the broker as a protocol string and comes back as JSON text, never as markup.

use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::get;
use serde::{Deserialize, Serialize};

use super::{error, json};
use crate::form::{get as field, parse, text};
use crate::gather::{DeleteGroupResult, OffsetDeleteResult, ResetResult};
use crate::model::{
    GroupDeleteView, GroupLagView, GroupRefused, GroupSpan, GroupStates, GroupsView,
    OffsetDeleteView, Refusal, ResetView, StoryView,
};
use crate::state::Shared;

/// `GET /api/v1/groups?q=&state=`: the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GroupsResponse {
    pub view: GroupsView,
    /// The states that are on (`stable`, `rebalancing`, `empty`); none means every group.
    pub states: Vec<String>,
    /// The line about a group that is rebalancing, as Rust words it (`groups_words::attention_sentence`).
    pub attention: Option<Vec<GroupSpan>>,
    /// The address the empty state's command points at.
    pub listen: String,
}

/// Kafka's refusal of a group write: its error name as the code and its own sentence, then the console's reading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GroupWriteRefusal {
    /// Kafka's error name: `NON_EMPTY_GROUP`.
    pub code: String,
    /// Kafka's words for it.
    pub message: String,
    /// Kafka's error number: `68`.
    pub number: i16,
    /// The rule, what would pass and the call that was refused (Reset offsets, Delete group).
    pub refused: Option<GroupRefused>,
    /// The dialog's data again (Delete offsets).
    pub offsets: Option<OffsetDeleteView>,
}

/// A choice the dialog cannot send: a field to fix, in the console's words, and the dialog's data again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GroupWriteInvalid {
    /// `INVALID`.
    pub code: String,
    pub message: String,
    pub reset: Option<ResetView>,
    pub offsets: Option<OffsetDeleteView>,
}

/// A write Kafka accepted: how many offsets moved or were forgotten (0 for a deleted group).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GroupWriteDone {
    pub done: usize,
}

/// `POST /api/v1/groups/{group}/reset`: what the Reset dialog sends (the preview's own choice).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ResetRequest {
    pub topic: String,
    /// `earliest`, `latest`, `offset`, `time`, `shift`, `current`.
    pub to: String,
    /// The offset, the shift, or the time (milliseconds since the epoch, or `2026-11-06 00:14`).
    pub value: String,
}

/// `POST /api/v1/groups/{group}/offsets/delete`: the topics whose committed offsets go.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OffsetDeleteRequest {
    pub topics: Vec<String>,
}

fn no_group(id: &str) -> Response {
    error(
        StatusCode::NOT_FOUND,
        "GROUP_NOT_FOUND",
        &format!(
            "The broker knows no group called {id}, or the console may not describe it. A group exists once a consumer joins or commits."
        ),
    )
}

fn fields(q: Option<&str>) -> Vec<(String, String)> {
    parse(q.unwrap_or(""))
}

async fn groups(State(s): State<Shared>, RawQuery(q): RawQuery) -> Response {
    let f = fields(q.as_deref());
    let filter: String = field(&f, "q")
        .unwrap_or("")
        .trim()
        .chars()
        .take(249)
        .collect();
    let state = GroupStates::parse(&f);
    let view = s.source.groups(&filter, &state, None).await;
    let attention = view
        .attention
        .as_ref()
        .map(crate::groups_words::attention_sentence);
    json(
        StatusCode::OK,
        &GroupsResponse {
            states: state.values().into_iter().map(str::to_owned).collect(),
            attention,
            listen: s.source.listen(),
            view,
        },
    )
}

async fn group(State(s): State<Shared>, Path(id): Path<String>, RawQuery(q): RawQuery) -> Response {
    let f = fields(q.as_deref());
    let expiring = field(&f, "expiring") == Some("1");
    match s.source.group(&id, expiring).await {
        Some(v) => json::<GroupLagView>(StatusCode::OK, &v),
        None => no_group(&id),
    }
}

async fn story(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    match s.source.story(&id).await {
        Some(v) => json::<StoryView>(StatusCode::OK, &v),
        None => no_group(&id),
    }
}

/// `GET /api/v1/groups/{group}/reset?topic=&to=&value=`: the preview. A GET: it reads OffsetFetch and ListOffsets and never
/// writes.
async fn reset_preview(
    State(s): State<Shared>,
    Path(id): Path<String>,
    RawQuery(q): RawQuery,
) -> Response {
    let f = fields(q.as_deref());
    let to = field(&f, "to").unwrap_or("latest");
    match s
        .source
        .reset_form(&id, text(&f, "topic"), to, text(&f, "value"))
        .await
    {
        Some(v) => json::<ResetView>(StatusCode::OK, &v),
        None => no_group(&id),
    }
}

fn refused(
    r: &Refusal,
    refused: Option<GroupRefused>,
    offsets: Option<OffsetDeleteView>,
) -> Response {
    json(
        StatusCode::CONFLICT,
        &GroupWriteRefusal {
            code: r.name.clone(),
            message: r.message.clone(),
            number: r.code,
            refused,
            offsets,
        },
    )
}

fn invalid(message: &str, reset: Option<ResetView>, offsets: Option<OffsetDeleteView>) -> Response {
    json(
        StatusCode::UNPROCESSABLE_ENTITY,
        &GroupWriteInvalid {
            code: "INVALID".to_owned(),
            message: message.to_owned(),
            reset,
            offsets,
        },
    )
}

/// The refusal of a body that is not what a write takes.
fn bad_body() -> Response {
    error(
        StatusCode::BAD_REQUEST,
        "BAD_REQUEST",
        "The request's body is not what this write takes.",
    )
}

async fn reset(
    State(s): State<Shared>,
    Path(id): Path<String>,
    bytes: axum::body::Bytes,
) -> Response {
    let Ok(req) = serde_json::from_slice::<ResetRequest>(&bytes) else {
        return bad_body();
    };
    match s
        .source
        .reset_offsets(&id, &req.topic, &req.to, &req.value)
        .await
    {
        ResetResult::Done(n) => json(StatusCode::OK, &GroupWriteDone { done: n }),
        ResetResult::Refused(r) => {
            let words = r.refusal.clone();
            refused(&words, Some(*r), None)
        }
        ResetResult::Invalid(v) => {
            let message = v.error.clone().unwrap_or_default();
            invalid(&message, Some(*v), None)
        }
        ResetResult::NoGroup => no_group(&id),
    }
}

/// `GET /api/v1/groups/{group}/delete`: what goes with the group.
async fn delete_preview(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    match s.source.delete_group_form(&id).await {
        Some(v) => json::<GroupDeleteView>(StatusCode::OK, &v),
        None => no_group(&id),
    }
}

async fn delete(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    match s.source.delete_group(&id).await {
        DeleteGroupResult::Done => json(StatusCode::OK, &GroupWriteDone { done: 0 }),
        DeleteGroupResult::Refused(r) => {
            let words = r.refusal.clone();
            refused(&words, Some(*r), None)
        }
        DeleteGroupResult::NoGroup => no_group(&id),
    }
}

fn chosen(f: &[(String, String)]) -> Vec<String> {
    f.iter()
        .filter(|(k, _)| k == "topic")
        .map(|(_, v)| v.clone())
        .collect()
}

/// `GET /api/v1/groups/{group}/offsets/delete?topic=..`: the topics with commits, those named ticked (all when none is).
async fn offsets_preview(
    State(s): State<Shared>,
    Path(id): Path<String>,
    RawQuery(q): RawQuery,
) -> Response {
    let f = fields(q.as_deref());
    match s.source.offset_delete_form(&id, &chosen(&f)).await {
        Some(v) => json::<OffsetDeleteView>(StatusCode::OK, &v),
        None => no_group(&id),
    }
}

async fn offsets_delete(
    State(s): State<Shared>,
    Path(id): Path<String>,
    bytes: axum::body::Bytes,
) -> Response {
    let Ok(req) = serde_json::from_slice::<OffsetDeleteRequest>(&bytes) else {
        return bad_body();
    };
    // No topic named is nothing to send (the form would tick every topic, which is not what was asked).
    if req.topics.is_empty() {
        return match s.source.offset_delete_form(&id, &[]).await {
            Some(v) => invalid("Choose a topic whose offsets go.", None, Some(v)),
            None => no_group(&id),
        };
    }
    match s.source.offset_delete(&id, &req.topics).await {
        OffsetDeleteResult::Done(n) => json(StatusCode::OK, &GroupWriteDone { done: n }),
        OffsetDeleteResult::Refused(v, r) => refused(&r, None, Some(*v)),
        OffsetDeleteResult::Nothing(v) => {
            invalid("Choose a topic whose offsets go.", None, Some(*v))
        }
        OffsetDeleteResult::NoGroup => no_group(&id),
    }
}

/// The routes of the Groups pages, under the console's root (`api::routes` puts the guard in front).
pub fn routes(web: &crate::base::Base) -> axum::Router<Shared> {
    axum::Router::new()
        .route(&web.url("/api/v1/groups"), get(groups))
        .route(&web.url("/api/v1/groups/{group}"), get(group))
        .route(&web.url("/api/v1/groups/{group}/story"), get(story))
        .route(
            &web.url("/api/v1/groups/{group}/reset"),
            get(reset_preview).post(reset),
        )
        .route(
            &web.url("/api/v1/groups/{group}/delete"),
            get(delete_preview).post(delete),
        )
        .route(
            &web.url("/api/v1/groups/{group}/offsets/delete"),
            get(offsets_preview).post(offsets_delete),
        )
}

/// The GET addresses of one group, over the fixtures' world (`analytics`, `fulfilment`, `search-indexer`); the tests read
/// them all.
pub const GROUP_GET_PATHS: &[&str] = &[
    "/api/v1/groups/search-indexer",
    "/api/v1/groups/analytics?expiring=1",
    "/api/v1/groups/fulfilment/story",
    "/api/v1/groups/analytics/reset?topic=orders&to=earliest",
    "/api/v1/groups/search-indexer/delete",
    "/api/v1/groups/analytics/offsets/delete",
];
