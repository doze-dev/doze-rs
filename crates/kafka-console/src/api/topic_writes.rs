//! The topic writes of the React console (slice R1b): Create topic, Produce, Add partitions, Delete records, Delete topic.
//!
//! Each dialog has a GET that answers its view-model for what was typed so far (the call beside the form, the teaching, a
//! button's label: the words are Rust's), and a POST (form-encoded, as the classic forms post, behind the same-origin
//! proof of [`super::guard`]) that does the write through the same `Source` the classic console uses. A POST answers:
//! - `{ outcome: "done", ... }` and what was made (the topic, the record, the new count);
//! - `{ outcome: "refused", code, message, detail }` when Kafka refused: the code Kafka's error name, the message the
//!   broker's own sentence, `detail` the refused state the page draws;
//! - `{ outcome: "invalid", code: "INVALID", message, detail }` when the form could not be sent (a number that is not
//!   one), `detail` the dialog again with its field errors;
//! - 404 when there is no such topic.
//!
//! Every answer to a write is a 200, as the group writes of R2 (`outcome`): a refusal is an answer, not a failure of the
//! request, and Chromium and WebKit log every non-2xx fetch as a console error, which the browser gate rightly forbids.
//! The client's `post` throws a refused or invalid outcome as an `ApiFailure` carrying `detail`.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::get;
use serde::Serialize;

use super::{error, json};
use crate::calls::{kcat_consume, kcat_produce};
use crate::form::{get as field, parse, parse_body, text};
use crate::model::{
    CreateRefused, CreateResult, CreateView, Created, Draft, ProduceInput, ProduceRefused,
    ProduceResult, ProduceView, Produced, Refusal, Span, Step, Teach, Template,
};
use crate::phrases::{acked, clock_ms};
use crate::state::Shared;
use crate::validate::retention_words;
use crate::write_words;

/// The most header rows a produce carries (as the classic dialog).
const MOST_HEADERS: usize = 8;

/// What a write made: the value with `outcome: "done"` beside its fields.
pub(super) fn done<T: Serialize>(value: &T) -> Response {
    #[derive(Serialize)]
    struct Body<'a, T: Serialize> {
        outcome: &'static str,
        #[serde(flatten)]
        value: &'a T,
    }
    json(
        StatusCode::OK,
        &Body {
            outcome: "done",
            value,
        },
    )
}

/// A refusal with the state the page draws: `{ outcome, code, message, detail }`.
fn refused_with<T: Serialize>(
    outcome: &'static str,
    code: &str,
    message: &str,
    detail: &T,
) -> Response {
    #[derive(Serialize)]
    struct Body<'a, T: Serialize> {
        outcome: &'static str,
        code: &'a str,
        message: &'a str,
        detail: &'a T,
    }
    json(
        StatusCode::OK,
        &Body {
            outcome,
            code,
            message,
            detail,
        },
    )
}

/// Kafka refused: its error name and the broker's sentence, and the state to draw.
pub(super) fn kafka_refused<T: Serialize>(r: &Refusal, detail: &T) -> Response {
    refused_with("refused", &r.name, &r.message, detail)
}

/// The form could not be sent: its first error, and the dialog again.
pub(super) fn invalid<T: Serialize>(message: &str, detail: &T) -> Response {
    refused_with("invalid", "INVALID", message, detail)
}

pub(super) fn no_topic(topic: &str) -> Response {
    let shown: String = topic.chars().take(80).collect();
    error(
        StatusCode::NOT_FOUND,
        "NOT_FOUND",
        &format!("There is no topic called {shown}."),
    )
}

pub(super) fn query(q: Option<&String>) -> Vec<(String, String)> {
    parse(q.map_or("", String::as_str))
}

// ---- Create topic -------------------------------------------------------------------------------------------------

/// `GET /api/v1/create-topic?step=&name=&partitions=&template=&cleanup.policy=&retention.ms=`: the form at a step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CreateForm {
    pub view: CreateView,
    /// `= 7 d` beside the retention figure, empty when it is not one.
    pub retention_words: String,
    /// The sentence under the title for this step.
    pub sub: String,
}

/// What a create made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CreatedAnswer {
    pub created: Created,
    /// `retention 7 d`, or the figure.
    pub retention: String,
    /// The two next steps, as terminal commands.
    pub produce: Vec<String>,
    pub consume: Vec<String>,
}

/// What a "Check only" said (200): every check passed and nothing was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CheckedAnswer {
    pub checked: bool,
    pub partitions: i32,
}

fn draft_of(fresh: Draft, f: &[(String, String)]) -> Draft {
    let mut d = fresh;
    let template = Template::from_id(text(f, "template"));
    let (cleanup, retention) = crate::gather::template_values(template);
    d.template = template;
    d.name = text(f, "name").to_owned();
    if let Some(p) = field(f, "partitions") {
        p.clone_into(&mut d.partitions);
    }
    field(f, "cleanup.policy")
        .unwrap_or(cleanup)
        .clone_into(&mut d.cleanup);
    field(f, "retention.ms")
        .unwrap_or(retention)
        .clone_into(&mut d.retention);
    d
}

fn step_sub(step: Step) -> &'static str {
    match step {
        Step::Name => {
            "Three steps. The call on the right is the one that will be sent; it changes as you type."
        }
        Step::Config => {
            "What the topic does with old records. Kafka's own sentence, the default and when it takes effect sit under each field."
        }
        Step::Review => "Check what will be sent, then create. Nothing exists until you do.",
    }
}

async fn create_form(State(s): State<Shared>, RawQuery(q): RawQuery) -> Response {
    let f = query(q.as_ref());
    let draft = draft_of(s.source.fresh_draft().await, &f);
    let view = s
        .source
        .create_form(draft, Step::from_id(text(&f, "step")))
        .await;
    json(
        StatusCode::OK,
        &CreateForm {
            retention_words: retention_words(&view.draft.retention),
            sub: step_sub(view.step).to_owned(),
            view,
        },
    )
}

fn first_error(v: &CreateView) -> String {
    v.errors
        .name
        .clone()
        .or_else(|| v.errors.partitions.clone())
        .or_else(|| v.errors.retention.clone())
        .unwrap_or_else(|| "The form is not complete.".to_owned())
}

/// `POST /api/v1/create-topic` (`mode=check` for "Check only", which sends `validate_only` and makes nothing).
async fn create(State(s): State<Shared>, body: Bytes) -> Response {
    let f = parse_body(&body);
    let draft = draft_of(s.source.fresh_draft().await, &f);
    let check = text(&f, "mode") == "check";
    match s.source.create_topic(draft, check).await {
        CreateResult::Created(c) => {
            let words = retention_words(&c.retention);
            done(&CreatedAnswer {
                retention: if words.is_empty() {
                    c.retention.clone()
                } else {
                    format!("retention {}", words.trim_start_matches("= "))
                },
                produce: kcat_produce(&c.address, &c.name),
                consume: kcat_consume(&c.address, &c.name),
                created: c,
            })
        }
        CreateResult::Checked(c) => done(&CheckedAnswer {
            checked: true,
            partitions: c.partitions,
        }),
        CreateResult::Refused(r) => kafka_refused::<CreateRefused>(&r.refusal.clone(), &r),
        CreateResult::Invalid(v) => invalid(&first_error(&v), &v),
    }
}

// ---- Produce ------------------------------------------------------------------------------------------------------

/// `GET /api/v1/topics/{topic}/produce?partition=&key=&value=&acks=&hn0=&hv0=...`: the dialog for what was typed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProduceForm {
    pub view: ProduceView,
    pub acks_teach: Teach,
    pub key_help: Vec<Span>,
}

/// A record the broker took (200), with the sentences of the board's banner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProducedAnswer {
    pub produced: Produced,
    /// `22:14:41.208` (UTC).
    pub clock: String,
    /// `acknowledged after the flush (acks -1)`.
    pub acked: String,
    /// Is it visible to read_committed: the lead and the sentence after it.
    pub visible_lead: String,
    pub visible: String,
}

/// A refused produce (the `detail` of a refused outcome).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProduceRefusedAnswer {
    pub refused: ProduceRefused,
    /// The sentence under the topic's name.
    pub sentence: String,
}

fn produce_input(f: &[(String, String)]) -> ProduceInput {
    let headers = (0..MOST_HEADERS)
        .map(|i| {
            (
                text(f, &format!("hn{i}")).to_owned(),
                text(f, &format!("hv{i}")).to_owned(),
            )
        })
        .filter(|(n, v)| !n.is_empty() || !v.is_empty())
        .collect();
    ProduceInput {
        partition: text(f, "partition").to_owned(),
        key: text(f, "key").to_owned(),
        value: text(f, "value").to_owned(),
        headers,
        acks: text(f, "acks").to_owned(),
    }
}

async fn produce_form(
    State(s): State<Shared>,
    Path(topic): Path<String>,
    RawQuery(q): RawQuery,
) -> Response {
    let input = produce_input(&query(q.as_ref()));
    let Some(view) = s.source.produce_form(&topic, &input).await else {
        return no_topic(&topic);
    };
    json(
        StatusCode::OK,
        &ProduceForm {
            acks_teach: write_words::acks_teach(view.transaction.as_deref()),
            key_help: write_words::key_help(&view),
            view,
        },
    )
}

fn visibility(p: &Produced) -> (String, String) {
    let n = crate::format::number;
    let v = &p.visibility;
    if p.row.committed {
        (
            "Yes, to both isolation levels.".to_owned(),
            format!(
                "{}-{} has no open transaction below this record: its last stable offset is {} and its high watermark {}. A consumer reading read_committed gets offset {} as soon as it asks.",
                p.topic,
                p.partition,
                n(v.last_stable),
                n(v.high_watermark),
                n(p.offset)
            ),
        )
    } else {
        (
            "To read_uncommitted now; to read_committed when the transaction ends.".to_owned(),
            format!(
                "{}-{} has an open transaction: its last stable offset is {}, below this record at {}. A consumer reading read_committed does not get the record until the transaction commits or aborts; the high watermark is {}.",
                p.topic,
                p.partition,
                n(v.last_stable),
                n(p.offset),
                n(v.high_watermark)
            ),
        )
    }
}

async fn produce(State(s): State<Shared>, Path(topic): Path<String>, body: Bytes) -> Response {
    let input = produce_input(&parse_body(&body));
    match s.source.produce(&topic, &input).await {
        ProduceResult::Sent(p) => {
            let (visible_lead, visible) = visibility(&p);
            done(&ProducedAnswer {
                clock: clock_ms(p.timestamp_ms),
                acked: acked(p.acks).to_owned(),
                visible_lead,
                visible,
                produced: *p,
            })
        }
        ProduceResult::Refused(r) => {
            let sentence = if r.view.compacts {
                "A compacted topic keeps the latest record for every key. Without a key a record has nothing to be the latest of."
            } else {
                "The record was not written. No offset was used."
            };
            kafka_refused(
                &r.refusal,
                &ProduceRefusedAnswer {
                    sentence: sentence.to_owned(),
                    refused: (*r).clone(),
                },
            )
        }
        ProduceResult::Invalid(v) => {
            let e = &v.errors;
            let first = e
                .partition
                .clone()
                .or_else(|| e.acks.clone())
                .or_else(|| e.key.clone())
                .unwrap_or_else(|| "The form is not complete.".to_owned());
            invalid(&first, &v)
        }
        ProduceResult::NoTopic => no_topic(&topic),
    }
}

/// The routes of the topic writes under `web`.
pub(super) fn routes(web: &crate::base::Base) -> Router<Shared> {
    Router::new()
        .route(
            &web.url("/api/v1/create-topic"),
            get(create_form).post(create),
        )
        .route(
            &web.url("/api/v1/topics/{topic}/produce"),
            get(produce_form).post(produce),
        )
}
