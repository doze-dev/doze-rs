//! The Story of a group: the page, the flapping rule held against the group, and one rebalance opened as a drawer over the
//! page. Every route is a plain GET that changes nothing; a group's name is hostile input, shown as text and put in links
//! only through `segment`.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};

use super::groups::{gone, trail};
use super::html;
use super::overlays::{self, Parent};
use super::topics::page_of;
use crate::model::StoryView;
use crate::pages::story as page;
use crate::state::Shared;
use crate::ui::shell::Rail;

/// The Story page as the parent of an overlay (a rebalance's drawer).
fn story_parent(s: &Shared, v: &StoryView) -> Parent {
    Parent {
        rail: Rail::Resources,
        title: "Story".to_owned(),
        crumbs: trail(&v.head.id, Some("Story")),
        watch: s.source.watching(crate::live_updates::STORY),
        main: page::story_page(v, &s.base),
    }
}

/// `/groups/{group}/story`
pub async fn story(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    let Some(view) = s.source.story(&id).await else {
        return gone(&s, &id).await;
    };
    html(&page_of(&s, story_parent(&s, &view)).await)
}

/// `/groups/{group}/story/flapping`
pub async fn flapping(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    let Some(view) = s.source.story(&id).await else {
        return gone(&s, &id).await;
    };
    let mut crumbs = trail(&id, Some("Story"));
    if let Some((label, path)) = crumbs.last_mut() {
        *path = Some(format!("/groups/{}/story", crate::base::segment(&id)));
        let _ = label;
    }
    crumbs.push(("Flapping".to_owned(), None));
    let parent = Parent {
        rail: Rail::Resources,
        title: "Flapping".to_owned(),
        crumbs,
        watch: &[],
        main: page::flapping_page(&view, &s.base),
    };
    html(&page_of(&s, parent).await)
}

/// `/groups/{group}/story/{generation}`: a drawer over the Story page.
pub async fn rebalance(
    State(s): State<Shared>,
    Path((id, generation)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let Some(view) = s.source.story(&id).await else {
        return gone(&s, &id).await;
    };
    let found = generation
        .parse::<i32>()
        .ok()
        .and_then(|n| view.generations.iter().find(|g| g.generation == n));
    let Some(g) = found else {
        return missing(&s, &uri).await;
    };
    let drawer = page::rebalance_drawer(&view, g, &s.base);
    if overlays::wants_fragment(&headers) {
        return overlays::fragment(StatusCode::OK, &drawer);
    }
    overlays::over_parent(&s, StatusCode::OK, story_parent(&s, &view), drawer).await
}

/// A rebalance the ring does not hold (it was let go, or there never was one): the page says so.
async fn missing(s: &Shared, uri: &Uri) -> Response {
    let main = crate::pages::not_found(uri.path(), &s.base);
    let doc = s
        .document(
            Rail::Resources,
            "No such rebalance",
            &[("Resources", Some("/topics")), ("No such rebalance", None)],
            &[],
            main,
        )
        .await;
    (StatusCode::NOT_FOUND, html(&doc)).into_response()
}

/// The fragments of the Story page, by id: what the stream tells it to fetch again; `None` for an id that is not one.
pub(crate) async fn fragment(s: &Shared, id: &str, query: &Option<String>) -> Option<maud::Markup> {
    use crate::story_ids::{STORY_NOW, STORY_SENTENCE};
    if id != STORY_NOW && id != STORY_SENTENCE {
        return None;
    }
    let f = crate::form::parse(query.as_deref().unwrap_or(""));
    let view = s.source.story(crate::form::get(&f, "group")?).await?;
    Some(if id == STORY_NOW {
        page::now_fragment(&view, &s.base)
    } else {
        page::sentence_fragment(&view, &s.base)
    })
}
