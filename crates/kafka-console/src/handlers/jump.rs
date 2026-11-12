//! Jump to a topic or a group: `/jump` is the palette to a script (the overlay alone) and a page of links to a browser.

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
    let groups = s.source.group_names().await;
    let total = all.total + groups.len();
    if wants_fragment(&headers) {
        let topics: Vec<String> = all.rows.iter().map(|r| r.name.clone()).collect();
        let items = pages::JumpItem::all(&topics, &groups);
        return fragment(StatusCode::OK, &pages::jump_overlay(&items, total, &s.base));
    }
    let (topics, groups): (Vec<String>, Vec<String>) = if filter.is_empty() {
        (all.rows.iter().map(|r| r.name.clone()).collect(), groups)
    } else {
        let want = filter.to_lowercase();
        let topics = s.source.topics(&filter, false, None).await;
        let groups = groups
            .into_iter()
            .filter(|g| g.to_lowercase().contains(&want))
            .collect();
        (topics.rows.iter().map(|r| r.name.clone()).collect(), groups)
    };
    let items = pages::JumpItem::all(&topics, &groups);
    let main = pages::jump_page(&items, total, &filter, &s.base);
    html(
        &s.document(
            Rail::Resources,
            "Jump to a topic or group",
            &[
                ("Resources", Some("/topics")),
                ("Jump to a topic or group", None),
            ],
            &[],
            main,
        )
        .await,
    )
}
