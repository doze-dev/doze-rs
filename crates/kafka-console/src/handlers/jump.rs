//! Jump to a topic: `/jump` is the palette to a script (the overlay alone) and a page of links to a browser.

use axum::extract::{RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;

use super::html;
use super::overlays::{fragment, wants_fragment};
use crate::pages;
use crate::state::Shared;
use crate::ui::shell::Rail;

/// `GET /jump?q=`
pub async fn jump(
    State(s): State<Shared>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let fields = crate::form::parse(query.as_deref().unwrap_or(""));
    let filter = crate::form::get(&fields, "q")
        .unwrap_or("")
        .trim()
        .to_owned();
    let all = s.source.topics("", false, None).await;
    if wants_fragment(&headers) {
        let names: Vec<String> = all.rows.iter().map(|r| r.name.clone()).collect();
        return fragment(
            StatusCode::OK,
            &pages::jump_overlay(&names, all.total, &s.base),
        );
    }
    let view = if filter.is_empty() {
        all
    } else {
        s.source.topics(&filter, false, None).await
    };
    let names: Vec<String> = view.rows.iter().map(|r| r.name.clone()).collect();
    let main = pages::jump_page(&names, view.total, &filter, &s.base);
    html(
        &s.document(
            Rail::Resources,
            "Jump to a topic",
            &[("Resources", Some("/topics")), ("Jump to a topic", None)],
            &[],
            main,
        )
        .await,
    )
}
