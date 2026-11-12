//! A screen of the design that this edition does not build: one designed page (`pages::unavailable`) with its own
//! breadcrumb and the way back, answered with HTTP 200. The way back is the page the visitor came from when the browser says
//! it was one of ours, else the section the screen belongs to.

use axum::extract::RawPathParams;
use axum::http::HeaderMap;
use axum::http::header::{HOST, REFERER};
use axum::response::Response;

use super::html;
use crate::base::{Base, segment};
use crate::pages;
use crate::state::Shared;
use crate::trail::{Place, TopicPage, borrowed};
use crate::ui::shell::Rail;

/// The path and query of `referer` when it is a page of this console: same authority as `host`, under the base, and not the
/// page being asked for. A `Referer` is whatever the browser (or a page that linked here) sent, so each part is checked.
#[must_use]
pub(super) fn back_path(
    referer: Option<&str>,
    host: Option<&str>,
    base: &Base,
    current: &str,
) -> Option<String> {
    let rest = referer?
        .strip_prefix("http://")
        .or_else(|| referer?.strip_prefix("https://"))?;
    let (authority, path) = rest.split_once('/')?;
    if !authority.eq_ignore_ascii_case(host?) {
        return None;
    }
    let path = format!("/{path}");
    let bare = path.split(['?', '#']).next().unwrap_or("");
    let unusable = bare == current
        || path.contains(['\r', '\n', '<', '>', '"', '\\'])
        || path.starts_with("//");
    (!unusable && base.strip(bare).is_some()).then_some(path)
}

/// What the screen is, from the route's name and its path parameters: where its breadcrumb goes and where "back" goes when
/// there is no page to return to.
fn place<'a>(
    name: &str,
    topic: &'a str,
    group: &'a str,
) -> (Place<'a>, Rail, String, &'static str) {
    match name {
        "Producers" => (
            Place::TopicPage(topic, TopicPage::Producers),
            Rail::Resources,
            format!("/topics/{}", segment(topic)),
            "Back to the topic",
        ),
        "Log" => (
            Place::TopicPage(topic, TopicPage::Log),
            Rail::Resources,
            format!("/topics/{}", segment(topic)),
            "Back to the topic",
        ),
        "Story" => (
            Place::GroupPage(group, "Story"),
            Rail::Resources,
            format!("/groups/{}", segment(group)),
            "Back to the group",
        ),
        "Access" => (Place::Access, Rail::Access, "/".to_owned(), "Back to Home"),
        "Lab" => (Place::Lab, Rail::Lab, "/".to_owned(), "Back to Home"),
        _ => (
            Place::Settings,
            Rail::Settings,
            "/".to_owned(),
            "Back to Home",
        ),
    }
}

/// The page of a route that no slice has built yet.
pub async fn unbuilt(
    s: Shared,
    name: &'static str,
    params: RawPathParams,
    headers: HeaderMap,
    current: &str,
) -> Response {
    let (mut topic, mut group) = (String::new(), String::new());
    for (key, value) in &params {
        match key {
            "topic" => value.clone_into(&mut topic),
            "group" => value.clone_into(&mut group),
            _ => {}
        }
    }
    let (at, rail, section, label) = place(name, &topic, &group);
    let text = |h| headers.get(h).and_then(|v| v.to_str().ok());
    let back = back_path(text(REFERER), text(HOST), &s.base, current);
    let (href, label) = match &back {
        Some(path) => (path.clone(), "Go back"),
        None => (s.base.url(&section), label),
    };
    let main = pages::unavailable(name, &href, label);
    let steps = at.steps();
    html(&s.document(rail, name, &borrowed(&steps), &[], main).await)
}

#[cfg(test)]
mod tests;
