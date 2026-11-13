//! The Lab's API (docs/port/phase9/lab.md): the page, one rule with the requests it touched, the rule form (choices, validation,
//! the server's preview), making, switching and removing a rule, the rehearsals that happen once (drop, expire, rebalance) and
//! Compact now. Every write is a POST behind the console's same-origin proof; the answers are the engine's or Kafka's words.
//! A console started with `--lab=false` (or the fixtures) answers every Lab address with `LAB_OFF`.

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::{Deserialize, Serialize};

use super::{error, json};
use crate::backend::Backend;
use crate::gather::lab as g;
use crate::lab::Lab;
use crate::model::lab::{
    LabCompact, LabFields, LabForm, LabHome, LabKind, LabMade, LabOnceDone, LabOnceForm, LabRule,
};
use crate::model::live::{LabDid, WireClass, WireRow};
use crate::recorder::Recorder;
use crate::source::Source;
use crate::state::Shared;

/// What the Lab needs of a live broker.
struct Parts<'a> {
    lab: &'a Lab,
    backend: &'a Backend,
    recorder: Option<&'a Recorder>,
}

fn parts(s: &Shared) -> Option<Parts<'_>> {
    match &s.source {
        Source::Live(l) => l.lab.as_deref().map(|lab| Parts {
            lab,
            backend: &l.backend,
            recorder: l.recorder.as_deref(),
        }),
        #[cfg(any(test, feature = "examples"))]
        Source::Fixture => None,
    }
}

fn off() -> Response {
    error(
        StatusCode::CONFLICT,
        "LAB_OFF",
        "The Lab is off on this broker (it was started with --lab=false), so no rule can be made.",
    )
}

fn bad(words: &str) -> Response {
    error(StatusCode::BAD_REQUEST, "BAD_REQUEST", words)
}

/// `GET /api/v1/lab`: the page, and the lists the forms choose from.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabResponse {
    pub home: LabHome,
}

/// A request a rule touched, as the rule's page lists it (the same row as on Live).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabHit {
    pub id: u64,
    pub at_ms: i64,
    pub client: String,
    pub tag: Option<String>,
    pub api: String,
    pub version: i16,
    pub transactional: bool,
    pub target: String,
    /// `done`, `failed`, `refused`, `closed`, ...
    pub class: String,
    pub code: i16,
    pub code_name: String,
    pub detail: String,
    pub took_us: u32,
    /// What the rule did to it, in words (`made the answer wait 400 ms`).
    pub did: String,
}

fn class_word(c: WireClass) -> &'static str {
    match c {
        WireClass::Done => "done",
        WireClass::Told => "told",
        WireClass::Failed => "failed",
        WireClass::Refused => "refused",
        WireClass::Denied => "denied",
        WireClass::Closed => "closed",
        #[allow(unreachable_patterns)]
        _ => "other",
    }
}

/// What a rule did to a request, in the words of the tag's explanation.
#[must_use]
pub fn did_words(d: LabDid, code_name: &str) -> String {
    match d {
        LabDid::Delayed(ms) => format!("made the answer wait {ms} ms more"),
        LabDid::Failed => format!("answered {code_name} without asking the broker"),
        LabDid::Dropped => "closed the connection instead of answering".to_owned(),
    }
}

fn hit_of(r: WireRow) -> LabHit {
    let did = r
        .lab
        .as_ref()
        .map_or_else(String::new, |l| did_words(l.what, &r.code_name));
    LabHit {
        id: r.id,
        at_ms: r.at_ms,
        client: r.client,
        tag: r.tag,
        api: r.api.to_owned(),
        version: r.version,
        transactional: r.transactional,
        target: r.target,
        class: class_word(r.class).to_owned(),
        code: r.code,
        code_name: r.code_name,
        detail: r.detail,
        took_us: r.took_us,
        did,
    }
}

/// `GET /api/v1/lab/rules/{id}`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabRuleResponse {
    pub rule: LabRule,
    pub now_ms: i64,
    pub hits: Vec<LabHit>,
    pub more_hits: bool,
}

/// What a rule's form posts, and what the preview reads.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LabRulePost {
    pub kind: String,
    pub fields: LabFields,
    /// Only the preview and the sentences, nothing made.
    pub check: bool,
}

/// What making a rule came to.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum LabMadeOutcome {
    /// The rule is in force: its number.
    Made { id: u32 },
    /// A field is wrong, or the engine refused: the form again with the sentences.
    Invalid { form: LabForm },
    /// Only the check was asked: the form with the preview or the sentences.
    Checked { form: LabForm },
}

/// What a rehearsal posts.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LabOncePost {
    pub target: String,
    /// For a drop: `once`, `1` or `5` (minutes of dropping).
    pub how: String,
}

/// What a rehearsal came to.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum LabOnceOutcome {
    Done,
    Invalid { form: Box<LabOnceForm> },
}

/// What Compact now posts.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LabCompactPost {
    pub topic: String,
    pub partition: i32,
}

/// What Compact now came to.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum LabCompactOutcome {
    Done,
    Refused { form: Box<LabCompact> },
}

/// Switching a rule on or off.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LabSwitchPost {
    pub on: bool,
}

fn kind_of(word: &str) -> Option<LabKind> {
    match word {
        "slow" => Some(LabKind::Slow),
        "fail" => Some(LabKind::Fail),
        _ => None,
    }
}

const MOST_TEXT: usize = 249;

fn cut(s: &str) -> String {
    s.trim().chars().take(MOST_TEXT).collect()
}

fn cut_fields(f: LabFields) -> LabFields {
    LabFields {
        api: cut(&f.api),
        topic: cut(&f.topic),
        partitions: cut(&f.partitions),
        group: cut(&f.group),
        client: cut(&f.client),
        delay: cut(&f.delay),
        jitter: cut(&f.jitter),
        share: cut(&f.share),
        ends: cut(&f.ends),
        hits: cut(&f.hits),
        code: cut(&f.code),
    }
}

pub(super) async fn home(State(s): State<Shared>) -> Response {
    let Some(p) = parts(&s) else {
        return json(
            StatusCode::OK,
            &LabResponse {
                home: LabHome {
                    off: true,
                    now_ms: crate::source::now_ms(),
                    active: Vec::new(),
                    ended: Vec::new(),
                    once: Vec::new(),
                    most_rules: 0,
                    compact_target: None,
                    compacted: Vec::new(),
                },
            },
        );
    };
    json(
        StatusCode::OK,
        &LabResponse {
            home: g::home(p.lab, p.backend).await,
        },
    )
}

pub(super) async fn rule(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    let Some(p) = parts(&s) else { return off() };
    let Some(v) = id
        .parse::<u32>()
        .ok()
        .and_then(|id| g::rule(p.lab, p.recorder, id))
    else {
        return error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "The Lab has no rule with that number: rules that ended more than an hour ago, or before the broker started, are gone.",
        );
    };
    json(
        StatusCode::OK,
        &LabRuleResponse {
            rule: v.rule,
            now_ms: v.now_ms,
            hits: v.hits.into_iter().map(hit_of).collect(),
            more_hits: v.more_hits,
        },
    )
}

/// `POST /api/v1/lab/rules`: makes the rule, or with `check` only answers the form with its preview.
pub(super) async fn make(State(s): State<Shared>, body: Bytes) -> Response {
    let Some(p) = parts(&s) else { return off() };
    let Ok(post) = serde_json::from_slice::<LabRulePost>(&body) else {
        return bad("The rule was not in the form the console sends.");
    };
    let Some(kind) = kind_of(&post.kind) else {
        return bad("A rule slows or fails requests: the kind is slow or fail.");
    };
    let fields = cut_fields(post.fields);
    if post.check {
        let form = g::rule_form(p.backend, p.recorder, kind, fields, true).await;
        return json(StatusCode::OK, &LabMadeOutcome::Checked { form });
    }
    let outcome = match g::create(p.lab, p.backend, p.recorder, kind, fields).await {
        LabMade::Made(id) => LabMadeOutcome::Made { id },
        LabMade::Invalid(form) => LabMadeOutcome::Invalid { form: *form },
    };
    json(StatusCode::OK, &outcome)
}

/// `GET /api/v1/lab/form?kind=`: the form as it opens (the board's defaults and the choices).
pub(super) async fn form(State(s): State<Shared>, RawQuery(q): RawQuery) -> Response {
    let Some(p) = parts(&s) else { return off() };
    let f = crate::form::parse(q.as_deref().unwrap_or(""));
    let kind = kind_of(crate::form::get(&f, "kind").unwrap_or("slow")).unwrap_or(LabKind::Slow);
    let fields = crate::lab::form::fields_of(&f);
    json(
        StatusCode::OK,
        &g::rule_form(p.backend, p.recorder, kind, fields, false).await,
    )
}

fn gone() -> Response {
    error(
        StatusCode::NOT_FOUND,
        "NOT_FOUND",
        "That rule is not in force any more: it ended or was removed.",
    )
}

pub(super) async fn switch(
    State(s): State<Shared>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let Some(p) = parts(&s) else { return off() };
    let (Ok(id), Ok(post)) = (
        id.parse::<u32>(),
        serde_json::from_slice::<LabSwitchPost>(&body),
    ) else {
        return bad("Say which rule, and whether it is on.");
    };
    if g::set_enabled(p.lab, id, post.on) {
        json(StatusCode::OK, &serde_json::json!({ "ok": true }))
    } else {
        gone()
    }
}

pub(super) async fn remove(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    let Some(p) = parts(&s) else { return off() };
    let Ok(id) = id.parse::<u32>() else {
        return bad("Say which rule.");
    };
    if g::remove(p.lab, id) {
        json(StatusCode::OK, &serde_json::json!({ "ok": true }))
    } else {
        gone()
    }
}

pub(super) async fn clear(State(s): State<Shared>) -> Response {
    let Some(p) = parts(&s) else { return off() };
    g::clear_history(p.lab);
    json(StatusCode::OK, &serde_json::json!({ "ok": true }))
}

/// `GET /api/v1/lab/once/{drop|expire|rebalance}?target=&how=`.
pub(super) async fn once_form(
    State(s): State<Shared>,
    Path(kind): Path<String>,
    RawQuery(q): RawQuery,
) -> Response {
    let Some(p) = parts(&s) else { return off() };
    let f = crate::form::parse(q.as_deref().unwrap_or(""));
    let target = cut(crate::form::text(&f, "target"));
    let how = cut(crate::form::text(&f, "how"));
    let v = match kind.as_str() {
        "drop" => g::drop_form(p.recorder, &target, &how),
        "expire" => g::expire_form(p.backend, &target).await,
        "rebalance" => g::rebalance_form(p.backend, &target).await,
        _ => return bad("The Lab rehearses drop, expire and rebalance."),
    };
    json(StatusCode::OK, &v)
}

pub(super) async fn once(
    State(s): State<Shared>,
    Path(kind): Path<String>,
    body: Bytes,
) -> Response {
    let Some(p) = parts(&s) else { return off() };
    let Ok(post) = serde_json::from_slice::<LabOncePost>(&body) else {
        return bad("The rehearsal was not in the form the console sends.");
    };
    let (target, how) = (cut(&post.target), cut(&post.how));
    let done = match kind.as_str() {
        "drop" => g::drop_connections(p.lab, p.recorder, &target, &how),
        "expire" => g::expire(p.lab, p.backend, &target).await,
        "rebalance" => g::rebalance(p.lab, p.backend, &target).await,
        _ => return bad("The Lab rehearses drop, expire and rebalance."),
    };
    let outcome = match done {
        LabOnceDone::Done => LabOnceOutcome::Done,
        LabOnceDone::Invalid(form) => LabOnceOutcome::Invalid { form },
    };
    json(StatusCode::OK, &outcome)
}

/// `GET /api/v1/lab/compact?topic=&partition=`.
pub(super) async fn compact_form(State(s): State<Shared>, RawQuery(q): RawQuery) -> Response {
    let f = crate::form::parse(q.as_deref().unwrap_or(""));
    let topic = cut(crate::form::text(&f, "topic"));
    let Ok(partition) = crate::form::text(&f, "partition").parse::<i32>() else {
        return bad("Say which partition, as a whole number.");
    };
    let (backend, on) = match &s.source {
        Source::Live(l) => (&l.backend, l.lab.is_some()),
        #[cfg(any(test, feature = "examples"))]
        Source::Fixture => return off(),
    };
    match g::compact_form(backend, on, &topic, partition).await {
        Some(v) => json(StatusCode::OK, &v),
        None => error(
            StatusCode::NOT_FOUND,
            "UNKNOWN_TOPIC_OR_PARTITION",
            "This server does not host this topic-partition.",
        ),
    }
}

pub(super) async fn compact(State(s): State<Shared>, body: Bytes) -> Response {
    let Some(p) = parts(&s) else { return off() };
    let Ok(post) = serde_json::from_slice::<LabCompactPost>(&body) else {
        return bad("Say which topic and partition.");
    };
    let outcome = match g::compact(p.lab, p.backend, &cut(&post.topic), post.partition).await {
        Ok(()) => LabCompactOutcome::Done,
        Err(form) => LabCompactOutcome::Refused { form },
    };
    json(StatusCode::OK, &outcome)
}
