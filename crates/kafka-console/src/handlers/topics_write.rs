//! Create topic: one route for the three steps (`?step=`), and the POST that makes the topic.

use axum::body::Bytes;
use axum::extract::{RawQuery, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use super::html;
use crate::form::{parse, parse_body, text};
use crate::gather;
use crate::model::{Checked, CreateResult, Draft, Step, Template};
use crate::pages;
use crate::state::Shared;
use crate::ui::shell::Rail;

/// What the fields of a form say the draft is: the starting point sets the configs unless the form carries its own.
pub(super) async fn draft_of(s: &Shared, fields: &[(String, String)]) -> Draft {
    let mut d = s.source.fresh_draft().await;
    let template = Template::from_id(text(fields, "template"));
    let (cleanup, retention) = gather::template_values(template);
    d.template = template;
    d.name = text(fields, "name").to_owned();
    if crate::form::get(fields, "partitions").is_some() {
        d.partitions = text(fields, "partitions").to_owned();
    }
    d.cleanup = crate::form::get(fields, "cleanup.policy")
        .map_or(cleanup, |v| v)
        .to_owned();
    d.retention = crate::form::get(fields, "retention.ms")
        .map_or(retention, |v| v)
        .to_owned();
    d
}

fn crumbs(step: Step) -> Vec<(&'static str, Option<&'static str>)> {
    let mut out = vec![("Topics", Some("/topics"))];
    match step {
        Step::Name => out.push(("Create topic", None)),
        Step::Config => out.extend([
            ("Create topic", Some("/topics/new")),
            ("Configuration", None),
        ]),
        Step::Review => out.extend([
            ("Create topic", Some("/topics/new")),
            ("Review and create", None),
        ]),
    }
    out
}

pub(super) async fn page(
    s: &Shared,
    status: StatusCode,
    title: &str,
    crumbs: &[(&str, Option<&str>)],
    main: maud::Markup,
) -> Response {
    let doc = s.document(Rail::Resources, title, crumbs, &[], main).await;
    (status, html(&doc)).into_response()
}

/// `GET /topics/new?step=...`
pub async fn create_form(State(s): State<Shared>, RawQuery(query): RawQuery) -> Response {
    let fields = parse(query.as_deref().unwrap_or(""));
    let wanted = Step::from_id(text(&fields, "step"));
    let draft = draft_of(&s, &fields).await;
    let view = s.source.create_form(draft, wanted).await;
    let title = match view.step {
        Step::Name => "Create topic",
        Step::Config => "Configuration",
        Step::Review => "Review and create",
    };
    page(
        &s,
        StatusCode::OK,
        title,
        &crumbs(view.step),
        pages::create_page(&view, &s.base),
    )
    .await
}

/// `POST /topics/new`: makes the topic ("Create topic"), or only checks ("Check only").
pub async fn create(State(s): State<Shared>, body: Bytes) -> Response {
    let fields = parse_body(&body);
    let draft = draft_of(&s, &fields).await;
    let check = text(&fields, "mode") == "check";
    match s.source.create_topic(draft.clone(), check).await {
        CreateResult::Created(c) => {
            let main = pages::created(&c, &s.base);
            let crumbs = [
                ("Topics", Some("/topics")),
                ("Create topic", Some("/topics/new")),
                ("Topic created", None),
            ];
            page(&s, StatusCode::OK, "Topic created", &crumbs, main).await
        }
        CreateResult::Checked(done @ Checked { .. }) => {
            let mut view = s.source.create_form(draft, Step::Review).await;
            view.checked = Some(done);
            page(
                &s,
                StatusCode::OK,
                "Review and create",
                &crumbs(Step::Review),
                pages::create_page(&view, &s.base),
            )
            .await
        }
        CreateResult::Refused(r) => {
            let crumbs = [
                ("Topics", Some("/topics")),
                ("Create topic", Some("/topics/new")),
                ("Create refused", None),
            ];
            page(
                &s,
                StatusCode::OK,
                "Create refused",
                &crumbs,
                pages::create_refused(&r, &s.base),
            )
            .await
        }
        CreateResult::Invalid(view) => {
            page(
                &s,
                StatusCode::UNPROCESSABLE_ENTITY,
                "Create topic",
                &crumbs(view.step),
                pages::create_page(&view, &s.base),
            )
            .await
        }
    }
}
