//! `GET /api/v1/topics/{topic}/messages`: a topic's records for the filters asked, and the cursor of the page after it.

use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;

use super::{MessagesResponse, fields, no_topic, valid};
use crate::api::json;
use crate::model::MessageQuery;
use crate::state::Shared;

pub(super) async fn messages(
    State(s): State<Shared>,
    Path(name): Path<String>,
    RawQuery(q): RawQuery,
) -> Response {
    if !valid(&name) {
        return no_topic(&name);
    }
    let (query, notes) = MessageQuery::parse(&fields(q.as_deref()));
    let Some(mut view) = s.source.messages(&name, query, notes).await else {
        return no_topic(&name);
    };
    s.source.schemas().decorate_rows(&mut view.rows, &name);
    let tailable = view.query.key.is_none()
        && view.query.offset.is_none()
        && view.query.time.is_none()
        && view.query.before.is_empty();
    let reach = if view.query.filtered() {
        crate::words::reach_sentence(&view)
    } else {
        Vec::new()
    };
    let older = (view.reach.more && view.query.limit < crate::model::MOST_LIMIT)
        .then(|| (view.query.limit * 2).min(crate::model::MOST_LIMIT));
    let earlier = earlier_cursor(&view);
    let cursor = view.rows.iter().map(|r| r.timestamp_ms).max().unwrap_or(0);
    json(
        StatusCode::OK,
        &MessagesResponse {
            view,
            reach,
            tailable,
            cursor,
            older,
            earlier,
        },
    )
}

/// The `before` cursor of the page after this one, when "Older" can no longer ask for a larger page: per partition, the
/// oldest offset shown, or where the last cursor stood for a partition this page showed nothing of.
fn earlier_cursor(view: &crate::model::MessagesView) -> Option<String> {
    let q = &view.query;
    if !view.reach.more
        || q.limit < crate::model::MOST_LIMIT
        || q.filtered()
        || view.rows.is_empty()
    {
        return None;
    }
    let mut at: Vec<(i32, i64)> = q.before.clone();
    for r in &view.rows {
        match at.iter_mut().find(|(p, _)| *p == r.partition) {
            Some((_, o)) => *o = (*o).min(r.offset),
            None => at.push((r.partition, r.offset)),
        }
    }
    at.sort_unstable();
    Some(crate::model::before_text(&at))
}
