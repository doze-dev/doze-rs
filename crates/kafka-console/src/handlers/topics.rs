//! The Topics pages: the list, a topic, its Messages, one record and its partitions. Every page is a plain GET whose filters
//! are in the address; a value that cannot be used is a sentence on the page.

use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use kafka_wire::ids::TopicName;

use super::html;
use crate::model::{MessageQuery, MessagesView, TopicHead};
use crate::pages::topics as page;
use crate::state::Shared;
use crate::stream::{TOPIC_IDS, TOPICS_IDS};
use crate::ui::shell::Rail;

type Steps<'a> = Vec<(&'a str, Option<&'a str>)>;

/// A topic name the broker could hold: anything else is no topic, whatever the broker would say.
fn valid(name: &str) -> bool {
    TopicName::parse(name).is_ok()
}

fn fields(query: &Option<String>) -> Vec<(String, String)> {
    crate::form::parse(query.as_deref().unwrap_or(""))
}

/// `/topics?q=..&internal=1`: the list.
pub async fn topics(State(s): State<Shared>, RawQuery(query): RawQuery) -> Response {
    let f = fields(&query);
    let filter = crate::form::get(&f, "q").unwrap_or("").trim().to_owned();
    let internal = crate::form::get(&f, "internal") == Some("1");
    let view = s
        .source
        .topics(&filter, internal, crate::form::get(&f, "state"))
        .await;
    let (title, rail_crumbs): (&str, Steps) = match view.state() {
        crate::model::TopicsState::None => (
            "No topics yet",
            vec![("Resources", Some("/topics")), ("Topics", None)],
        ),
        crate::model::TopicsState::Filtered => (
            "No topic matches",
            vec![("Resources", Some("/topics")), ("Topics", None)],
        ),
        crate::model::TopicsState::List => (
            "Topics",
            vec![("Resources", Some("/topics")), ("Topics", None)],
        ),
    };
    let q = crate::form::encode_query(&[
        ("q", &filter),
        ("internal", if internal { "1" } else { "" }),
    ]);
    let main = page::list_page(&view, &s.base, &s.source.listen(), &q);
    html(
        &s.document(
            Rail::Resources,
            title,
            &rail_crumbs,
            s.source.watching(TOPICS_IDS),
            main,
        )
        .await,
    )
}

async fn not_found(s: &Shared, name: &str) -> Response {
    let main = page::unknown(name, &s.base);
    let shown: String = name.chars().take(60).collect();
    let page = s
        .document(
            Rail::Resources,
            "No such topic",
            &[
                ("Resources", Some("/topics")),
                ("Topics", Some("/topics")),
                (&shown, None),
            ],
            &[],
            main,
        )
        .await;
    (StatusCode::NOT_FOUND, html(&page)).into_response()
}

/// The title of the state the Messages page is in, for the last step of the breadcrumb.
fn state_title(v: &MessagesView) -> Option<String> {
    use crate::format::number;
    let q = &v.query;
    if v.head.is_empty() && !q.filtered() {
        return None;
    }
    if !v.head.offline().is_empty()
        && !q.filtered()
        && q.reading == crate::model::Reading::Uncommitted
    {
        let down = v.head.offline();
        return Some(format!(
            "{} partition {} offline",
            v.head.name,
            crate::words::list_and(&down.iter().map(|p| p.partition).collect::<Vec<_>>())
        ));
    }
    if q.reading == crate::model::Reading::Committed {
        return Some("read_committed".to_owned());
    }
    if !q.filtered() {
        return None;
    }
    Some(match (q.partition, q.offset) {
        (Some(p), Some(o)) => format!("{}-{p} from {}", v.head.name, number(o)),
        (Some(p), None) => format!("{}-{p}", v.head.name),
        _ => "Filtered".to_owned(),
    })
}

async fn messages_response(s: Shared, name: String, query: Option<String>) -> Response {
    if !valid(&name) {
        return not_found(&s, &name).await;
    }
    let raw = fields(&query);
    let (q, notes) = MessageQuery::parse(&raw);
    let Some(view) = s.source.messages(&name, q, notes).await else {
        return not_found(&s, &name).await;
    };
    let q = view.query.to_query();
    let main = page::messages_page(&view, &s.base, &s.source.listen(), &q);
    let last = state_title(&view);
    // The plain Messages page carries the topic's wire dock (slice 9e): the last requests that touched it.
    let docked = last.is_none()
        && !view.head.is_empty()
        && view.head.offline().is_empty()
        && view.query.reading == crate::model::Reading::Uncommitted
        && !view.query.filtered();
    let main = if docked {
        let (rows, window, now) = s.source.topic_wire(&name).await;
        maud::html! { (main) (crate::pages::live::topic_dock(&name, &rows, &window, now, &s.base, false)) }
    } else {
        main
    };
    let t = page::topic_url(&s.base, &name);
    let t = s.base.strip(&t).unwrap_or("/topics").to_owned();
    let shown = view.head.name.clone();
    let mut steps: Steps = vec![("Resources", Some("/topics")), ("Topics", Some("/topics"))];
    let title = match &last {
        Some(l) => {
            steps.push((&shown, Some(&t)));
            steps.push((l, None));
            l.clone()
        }
        None => {
            steps.push((&shown, None));
            shown.clone()
        }
    };
    html(
        &s.document(
            Rail::Resources,
            &title,
            &steps,
            s.source.watching(if docked {
                crate::live_ids::TOPIC_DETAIL_IDS
            } else {
                TOPIC_IDS
            }),
            main,
        )
        .await,
    )
}

/// `/topics/{topic}`: the Messages tab with no filter.
pub async fn topic(
    State(s): State<Shared>,
    Path(name): Path<String>,
    RawQuery(query): RawQuery,
) -> Response {
    messages_response(s, name, query).await
}

/// `/topics/{topic}/messages`: the same page, with the filters it was asked for.
pub async fn messages(
    State(s): State<Shared>,
    Path(name): Path<String>,
    RawQuery(query): RawQuery,
) -> Response {
    messages_response(s, name, query).await
}

/// `/topics/{topic}/messages/{partition}/{offset}`.
pub async fn record(
    State(s): State<Shared>,
    Path((name, partition, offset)): Path<(String, String, String)>,
) -> Response {
    let (Some(p), Some(o)) = (
        partition.parse::<i32>().ok().filter(|p| *p >= 0),
        offset.parse::<i64>().ok().filter(|o| *o >= 0),
    ) else {
        return not_found(&s, &name).await;
    };
    if !valid(&name) {
        return not_found(&s, &name).await;
    }
    let Some(view) = s.source.record(&name, p, o).await else {
        return gone(&s, &name, p, o).await;
    };
    let main = page::record_page(&view, &s.base);
    let title = format!("{name}-{p} offset {}", crate::format::number(o));
    let t = page::topic_url(&s.base, &name);
    let t = s.base.strip(&t).unwrap_or("/topics").to_owned();
    let steps: Steps = vec![
        ("Resources", Some("/topics")),
        ("Topics", Some("/topics")),
        (&name, Some(&t)),
        (&title, None),
    ];
    html(&s.document(Rail::Resources, &title, &steps, &[], main).await)
}

async fn gone(s: &Shared, name: &str, p: i32, o: i64) -> Response {
    let main = page::no_record(name, p, o, &s.base);
    let shown: String = name.chars().take(60).collect();
    let page = s
        .document(
            Rail::Resources,
            "No such record",
            &[
                ("Resources", Some("/topics")),
                ("Topics", Some("/topics")),
                (&shown, None),
            ],
            &[],
            main,
        )
        .await;
    (StatusCode::NOT_FOUND, html(&page)).into_response()
}

/// `/topics/{topic}/partitions`.
pub async fn partitions(State(s): State<Shared>, Path(name): Path<String>) -> Response {
    if !valid(&name) {
        return not_found(&s, &name).await;
    }
    let Some(head): Option<TopicHead> = s.source.topic_head(&name).await else {
        return not_found(&s, &name).await;
    };
    let main = page::partitions_page(&head, &s.base);
    let t = page::topic_url(&s.base, &name);
    let t = s.base.strip(&t).unwrap_or("/topics").to_owned();
    let steps: Steps = vec![
        ("Resources", Some("/topics")),
        ("Topics", Some("/topics")),
        (&name, Some(&t)),
        ("Partitions", None),
    ];
    html(
        &s.document(Rail::Resources, "Partitions", &steps, &[], main)
            .await,
    )
}
