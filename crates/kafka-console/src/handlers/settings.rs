//! The Settings page, the edit dialog of a dynamic broker config and its POST. The dialog is an overlay (`ui::overlay`) over the
//! Settings page: its route answers the dialog alone to a script (`X-Fragment`) and the Settings page with the dialog open on
//! it to a browser without one. A GET never changes anything; every change is a POST with the same-origin proof.

use axum::body::Bytes;
use axum::extract::{RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use maud::Markup;

use super::html;
use super::overlays::{self, Parent};
use crate::form::{get, parse, parse_body, text};
use crate::gather::settings_edit::SettingsFields;
use crate::model::{SettingsEditView, SettingsResult};
use crate::pages;
use crate::state::Shared;
use crate::trail::{Place, borrowed};
use crate::ui::shell::Rail;

/// The longest text a field of the form is read to.
const MOST: usize = 200;

fn cut(s: &str) -> String {
    s.chars().take(MOST).collect()
}

/// A page of Settings: what became of an edit (saved, refused) is the page's own words, not a step of the trail.
async fn shell(s: &Shared, status: StatusCode, _tail: Option<&str>, main: Markup) -> Response {
    let steps = Place::Settings.steps();
    let doc = s
        .document(Rail::Settings, "Settings", &borrowed(&steps), &[], main)
        .await;
    (status, html(&doc)).into_response()
}

async fn no_config(s: &Shared, key: &str) -> Response {
    let shown: String = key.chars().take(80).collect();
    let main = maud::html! {
        (crate::ui::layout::page_header("No such config", maud::html! {
            p class="sub" { "The broker has no dynamic config called " span class="mono" { (shown) } ". Five can change while it runs; they are on Settings." }
        }, maud::html! {}))
        div class="row" { a class="btn pri" href=(pages::settings_url(&s.base)) { "Back to Settings" } }
    };
    shell(s, StatusCode::NOT_FOUND, Some("No such config"), main).await
}

/// The Settings page as the parent of an overlay (the edit dialog); its own handler is `page_of(parent)`.
async fn settings_parent(s: &Shared) -> Parent {
    let view = s.source.settings().await;
    Parent {
        rail: Rail::Settings,
        title: "Settings".to_owned(),
        crumbs: Place::Settings.steps(),
        watch: &[],
        main: pages::settings_page(&view, &s.base),
    }
}

/// A dialog: alone for a script (which opens it over the page it is on), else the Settings page with the dialog open on it.
async fn dialog(s: &Shared, status: StatusCode, h: &HeaderMap, v: &SettingsEditView) -> Response {
    let overlay = pages::settings_edit_dialog(v, &s.base);
    if overlays::wants_fragment(h) {
        return overlays::fragment(status, &overlay);
    }
    overlays::over_parent(s, status, settings_parent(s).await, overlay).await
}

/// `GET /settings`
pub async fn settings(State(s): State<Shared>) -> Response {
    let parent = settings_parent(&s).await;
    html(&super::topics::page_of(&s, parent).await)
}

/// `GET /settings/config/edit?key=`
pub async fn settings_edit_form(
    State(s): State<Shared>,
    RawQuery(q): RawQuery,
    headers: HeaderMap,
) -> Response {
    let f = parse(q.as_deref().unwrap_or(""));
    let key = cut(get(&f, "key").unwrap_or(""));
    match s
        .source
        .settings_form(&key, get(&f, "scope").unwrap_or("all"))
        .await
    {
        Some(v) => dialog(&s, StatusCode::OK, &headers, &v).await,
        None => no_config(&s, &key).await,
    }
}

/// What the POST carries. `probe` is the button that asks Kafka for a dry run of a config that cannot change.
struct Posted {
    key: String,
    was: String,
    scope: String,
    value: String,
    check: bool,
}

fn posted(body: &[u8]) -> Posted {
    let f = parse_body(body);
    let probe = cut(text(&f, "probe"));
    if !probe.is_empty() {
        return Posted {
            was: probe.clone(),
            key: probe,
            scope: "all".to_owned(),
            value: "3".to_owned(),
            check: true,
        };
    }
    let key = cut(text(&f, "key"));
    let was = cut(text(&f, "was"));
    Posted {
        was: if was.is_empty() { key.clone() } else { was },
        key,
        scope: cut(text(&f, "scope")),
        value: cut(text(&f, "value")),
        check: get(&f, "check") == Some("1"),
    }
}

/// `POST /settings/config/edit`
pub async fn settings_edit(State(s): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    let p = posted(&body);
    let fields = SettingsFields {
        key: &p.key,
        was: &p.was,
        scope: &p.scope,
        value: &p.value,
    };
    match s.source.settings_save(fields, p.check).await {
        SettingsResult::Saved(v) => {
            shell(
                &s,
                StatusCode::OK,
                Some("Saved"),
                pages::settings_saved(&v, &s.base),
            )
            .await
        }
        SettingsResult::Refused(r) => {
            shell(
                &s,
                StatusCode::OK,
                Some("Refused"),
                pages::settings_refused(&r, &s.base),
            )
            .await
        }
        SettingsResult::Checked(v) | SettingsResult::Switched(v) => {
            dialog(&s, StatusCode::OK, &headers, &v).await
        }
        SettingsResult::Invalid(v) => {
            dialog(&s, StatusCode::UNPROCESSABLE_ENTITY, &headers, &v).await
        }
    }
}
