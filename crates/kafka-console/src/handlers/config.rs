//! The Config pages of a topic: the list, the edit dialog and its POST, the reset dialog and its POST. A dialog is an
//! overlay (`ui::overlay`) over the Config page: its route answers the dialog alone to a script (`X-Fragment`) and the Config
//! page with the dialog open on it to a browser without one. A GET never changes anything; every change is a POST with the same-origin proof.

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use kafka_wire::ids::TopicName;
use maud::Markup;

use super::html;
use super::overlays::{self, Parent};
use crate::base::segment;
use crate::form::{get, parse, parse_body, text};
use crate::gather::{ConfigFields, ConfigMissing};
use crate::model::{ConfigAsk, ConfigResetResult, ConfigResult, ConfigShow};
use crate::pages;
use crate::state::Shared;
use crate::ui::shell::Rail;

/// The most rows a page may be asked to open, and the longest name of one.
const MOST_OPEN: usize = 8;
const MOST_KEY: usize = 120;

/// A topic name the broker could hold: anything else is no topic, whatever the broker would say.
fn valid(name: &str) -> bool {
    TopicName::parse(name).is_ok()
}

fn ask_of(query: &Option<String>) -> ConfigAsk {
    let f = parse(query.as_deref().unwrap_or(""));
    ConfigAsk {
        q: get(&f, "q")
            .unwrap_or("")
            .trim()
            .chars()
            .take(MOST_KEY)
            .collect(),
        show: ConfigShow::from_word(get(&f, "show").unwrap_or("")),
        open: get(&f, "open")
            .unwrap_or("")
            .split(',')
            .filter(|t| !t.is_empty() && t.len() <= MOST_KEY)
            .take(MOST_OPEN)
            .map(str::to_owned)
            .collect(),
        replication: get(&f, "replication") == Some("1"),
    }
}

/// A page in the shell, under `Resources > Topics > topic > Config > tail`.
async fn shell(
    s: &Shared,
    status: StatusCode,
    topic: &str,
    tail: Option<&str>,
    main: Markup,
) -> Response {
    let t = format!("/topics/{}", segment(topic));
    let c = format!("{t}/config");
    let mut steps: Vec<(&str, Option<&str>)> = vec![
        ("Resources", Some("/topics")),
        ("Topics", Some("/topics")),
        (topic, Some(&t)),
    ];
    let title = tail.unwrap_or("Config");
    steps.push(("Config", tail.map(|_| c.as_str())));
    if let Some(label) = tail {
        steps.push((label, None));
    }
    let doc = s.document(Rail::Resources, title, &steps, &[], main).await;
    (status, html(&doc)).into_response()
}

async fn no_topic(s: &Shared, topic: &str) -> Response {
    let shown: String = topic.chars().take(80).collect();
    let main = pages::no_topic(&shown, &s.base);
    let doc = s
        .document(
            Rail::Resources,
            "No such topic",
            &[
                ("Resources", Some("/topics")),
                ("Topics", Some("/topics")),
                ("No such topic", None),
            ],
            &[],
            main,
        )
        .await;
    (StatusCode::NOT_FOUND, html(&doc)).into_response()
}

async fn no_key(s: &Shared, topic: &str, key: &str) -> Response {
    let shown: String = key.chars().take(80).collect();
    let main = pages::config_no_key(topic, &shown, &s.base);
    shell(
        s,
        StatusCode::NOT_FOUND,
        topic,
        Some("No such config"),
        main,
    )
    .await
}

async fn missing(s: &Shared, topic: &str, key: &str, why: ConfigMissing) -> Response {
    match why {
        ConfigMissing::Topic => no_topic(s, topic).await,
        ConfigMissing::Key => no_key(s, topic, key).await,
    }
}

/// The Config page of a topic as the parent of an overlay (the edit and reset dialogs). `None` when there is no such topic.
async fn config_parent(s: &Shared, topic: &str, ask: ConfigAsk) -> Option<Parent> {
    let page = s.source.config_page(topic, ask).await?;
    let t = format!("/topics/{}", segment(topic));
    Some(Parent {
        rail: Rail::Resources,
        title: "Config".to_owned(),
        crumbs: vec![
            ("Resources".to_owned(), Some("/topics".to_owned())),
            ("Topics".to_owned(), Some("/topics".to_owned())),
            (topic.to_owned(), Some(t)),
            ("Config".to_owned(), None),
        ],
        watch: &[],
        main: pages::config_page(&page, &s.base),
    })
}

/// A dialog: alone for a script (which opens it over the page it is on), else the Config page with the dialog open on it.
async fn dialog(
    s: &Shared,
    status: StatusCode,
    h: &HeaderMap,
    topic: &str,
    overlay: Markup,
) -> Response {
    if overlays::wants_fragment(h) {
        return overlays::fragment(status, &overlay);
    }
    match config_parent(s, topic, ConfigAsk::default()).await {
        Some(parent) => overlays::over_parent(s, status, parent, overlay).await,
        None => no_topic(s, topic).await,
    }
}

/// `GET /topics/{topic}/config`
pub async fn config(
    State(s): State<Shared>,
    Path(topic): Path<String>,
    RawQuery(q): RawQuery,
) -> Response {
    if !valid(&topic) {
        return no_topic(&s, &topic).await;
    }
    let Some(parent) = config_parent(&s, &topic, ask_of(&q)).await else {
        return no_topic(&s, &topic).await;
    };
    html(&super::topics::page_of(&s, parent).await)
}

/// `GET /topics/{topic}/config/edit?key=`
pub async fn config_edit_form(
    State(s): State<Shared>,
    Path(topic): Path<String>,
    RawQuery(q): RawQuery,
    headers: HeaderMap,
) -> Response {
    let f = parse(q.as_deref().unwrap_or(""));
    let key = get(&f, "key").unwrap_or("");
    let fields = ConfigFields {
        key,
        op: get(&f, "op"),
        value: get(&f, "value"),
        other: get(&f, "other"),
    };
    if !valid(&topic) {
        return no_topic(&s, &topic).await;
    }
    match s.source.config_form(&topic, fields).await {
        Ok(v) => {
            let overlay = pages::config_edit_dialog(&v, &s.base);
            dialog(&s, StatusCode::OK, &headers, &topic, overlay).await
        }
        Err(why) => missing(&s, &topic, key, why).await,
    }
}

/// `POST /topics/{topic}/config/edit`
pub async fn config_edit(
    State(s): State<Shared>,
    Path(topic): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let f = parse_body(&body);
    let key = text(&f, "key");
    let fields = ConfigFields {
        key,
        op: Some(text(&f, "op")),
        value: Some(text(&f, "value")),
        other: Some(text(&f, "other")),
    };
    if !valid(&topic) {
        return no_topic(&s, &topic).await;
    }
    let check = get(&f, "check") == Some("1");
    match s.source.config_save(&topic, fields, check).await {
        ConfigResult::Saved(v) => {
            shell(
                &s,
                StatusCode::OK,
                &topic,
                Some("Saved"),
                pages::config_saved(&v, &s.base),
            )
            .await
        }
        ConfigResult::Refused(r) => {
            shell(
                &s,
                StatusCode::OK,
                &topic,
                Some("Refused"),
                pages::config_refused(&r, &s.base),
            )
            .await
        }
        ConfigResult::Checked(v) => {
            let overlay = pages::config_edit_dialog(&v, &s.base);
            dialog(&s, StatusCode::OK, &headers, &topic, overlay).await
        }
        ConfigResult::Invalid(v) => {
            let overlay = pages::config_edit_dialog(&v, &s.base);
            dialog(
                &s,
                StatusCode::UNPROCESSABLE_ENTITY,
                &headers,
                &topic,
                overlay,
            )
            .await
        }
        ConfigResult::NoTopic => no_topic(&s, &topic).await,
        ConfigResult::NoKey => no_key(&s, &topic, key).await,
    }
}

/// `GET /topics/{topic}/config/reset?key=`
pub async fn config_reset_form(
    State(s): State<Shared>,
    Path(topic): Path<String>,
    RawQuery(q): RawQuery,
    headers: HeaderMap,
) -> Response {
    let f = parse(q.as_deref().unwrap_or(""));
    let key = get(&f, "key").unwrap_or("");
    if !valid(&topic) {
        return no_topic(&s, &topic).await;
    }
    match s.source.config_reset_form(&topic, key).await {
        Ok(v) => {
            let overlay = pages::config_reset_dialog(&v, &s.base);
            dialog(&s, StatusCode::OK, &headers, &topic, overlay).await
        }
        Err(why) => missing(&s, &topic, key, why).await,
    }
}

/// `POST /topics/{topic}/config/reset`
pub async fn config_reset(
    State(s): State<Shared>,
    Path(topic): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let f = parse_body(&body);
    let key = text(&f, "key");
    if !valid(&topic) {
        return no_topic(&s, &topic).await;
    }
    match s.source.config_reset(&topic, key).await {
        ConfigResetResult::Done => {
            Redirect::to(&pages::config_url(&s.base, &topic, "")).into_response()
        }
        ConfigResetResult::Refused(v) => {
            let overlay = pages::config_reset_dialog(&v, &s.base);
            dialog(&s, StatusCode::OK, &headers, &topic, overlay).await
        }
        ConfigResetResult::NoTopic => no_topic(&s, &topic).await,
        ConfigResetResult::NoKey => no_key(&s, &topic, key).await,
    }
}
