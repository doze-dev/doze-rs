//! The handlers: each asks the source for a model, renders it, and answers.

mod fragments;
mod groups;
mod groups_fragments;
mod pages;
mod plain;
mod topics;
mod topics_dialogs;
mod topics_write;

pub use fragments::{check, fragment};
pub use groups::{
    delete as group_delete, delete_form as group_delete_form, group as group_page,
    groups as groups_list, lag as group_lag, member as group_member, members as group_members,
    offset_delete as offsets_delete, offset_delete_form as offsets_delete_form,
    reset as group_reset, reset_form as group_reset_form,
};
pub use groups_fragments::groups_fragment;
pub use pages::{advertised, connect, home, method_not_allowed, not_found, planned};
pub use plain::{events, health, static_file};
pub use topics::{
    messages as topic_messages, partitions as topic_partitions, record as topic_record, topic,
    topics,
};
pub use topics_dialogs::{
    delete, delete_form, delete_records, delete_records_form, partitions_add, partitions_add_form,
    produce, produce_form,
};
pub use topics_write::{create, create_form};

use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, ETAG};
use axum::http::{HeaderMap, StatusCode, header::IF_NONE_MATCH};
use axum::response::{IntoResponse, Response};
use maud::Markup;

use crate::assets::fingerprint;

/// A page or a fragment as an HTML answer.
#[must_use]
pub fn html(markup: &Markup) -> Response {
    (
        [(CONTENT_TYPE, "text/html; charset=utf-8")],
        markup.0.clone(),
    )
        .into_response()
}

/// Whether the request's `If-None-Match` names `tag` (or is `*`): the browser already has this.
#[must_use]
pub fn has_tag(headers: &HeaderMap, tag: &str) -> bool {
    headers
        .get(IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(',')
                .map(str::trim)
                .any(|t| t == "*" || t.trim_start_matches("W/") == tag)
        })
}

/// A fragment with an ETag over its text: a browser that has it already gets a 304 and nothing is swapped.
#[must_use]
pub fn etagged(headers: &HeaderMap, markup: &Markup) -> Response {
    let tag = format!("\"{}\"", fingerprint(markup.0.as_bytes()));
    let mut res = if has_tag(headers, &tag) {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        html(markup)
    };
    res.headers_mut().insert(
        ETAG,
        tag.parse()
            .unwrap_or_else(|_| axum::http::HeaderValue::from_static("\"0\"")),
    );
    res.headers_mut().insert(
        CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-cache"),
    );
    res
}
