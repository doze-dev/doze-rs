//! The Config pages of a topic: the list, the edit dialog and its POST, the reset dialog and its POST. A dialog is a route
//! that answers its body alone to a script (`X-Fragment`) and the whole page with the dialog in it to a browser without
//! one. A GET never changes anything; every change is a POST with the same-origin proof.

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use kafka_wire::ids::TopicName;
use maud::Markup;

use super::html;
use crate::base::segment;
use crate::form::{get, parse, parse_body, text};
use crate::gather::{ConfigFields, ConfigMissing};
use crate::model::{ConfigAsk, ConfigResetResult, ConfigResult, ConfigShow};
use crate::pages;
use crate::state::Shared;
use crate::ui::client::HEADER_FRAGMENT;
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

/// A dialog: alone for a script, else on a page of its own under its breadcrumb.
async fn dialog(
    s: &Shared,
    status: StatusCode,
    h: &HeaderMap,
    at: (&str, &str),
    make: impl Fn(bool) -> Markup,
) -> Response {
    if h.contains_key(HEADER_FRAGMENT) {
        return (status, html(&make(false))).into_response();
    }
    let (topic, title) = at;
    shell(s, status, topic, Some(title), make(true)).await
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
    let Some(page) = s.source.config_page(&topic, ask_of(&q)).await else {
        return no_topic(&s, &topic).await;
    };
    shell(
        &s,
        StatusCode::OK,
        &topic,
        None,
        pages::config_page(&page, &s.base),
    )
    .await
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
            let title = pages::config_dialog_title(&v);
            dialog(&s, StatusCode::OK, &headers, (&topic, &title), |inline| {
                pages::config_edit_dialog(&v, &s.base, inline)
            })
            .await
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
            let title = pages::config_dialog_title(&v);
            dialog(&s, StatusCode::OK, &headers, (&topic, &title), |inline| {
                pages::config_edit_dialog(&v, &s.base, inline)
            })
            .await
        }
        ConfigResult::Invalid(v) => {
            let title = pages::config_dialog_title(&v);
            dialog(
                &s,
                StatusCode::UNPROCESSABLE_ENTITY,
                &headers,
                (&topic, &title),
                |inline| pages::config_edit_dialog(&v, &s.base, inline),
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
            dialog(
                &s,
                StatusCode::OK,
                &headers,
                (&topic, "Reset to default"),
                |inline| pages::config_reset_dialog(&v, &s.base, inline),
            )
            .await
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
            dialog(
                &s,
                StatusCode::OK,
                &headers,
                (&topic, "Reset to default"),
                |inline| pages::config_reset_dialog(&v, &s.base, inline),
            )
            .await
        }
        ConfigResetResult::NoTopic => no_topic(&s, &topic).await,
        ConfigResetResult::NoKey => no_key(&s, &topic, key).await,
    }
}
