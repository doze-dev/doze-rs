//! The Log (with the Lab's Compact now), Producers and transaction answers of the Topics API (slice R1a).

use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::Serialize;

use super::{fields, no_topic, partition_of, valid};
use crate::api::{error, json};
use crate::log_ruler::Ruler;
use crate::log_words::{Expiry, Rule};
use crate::model::{LogState, LogView, ProducersView, Span, TxnView};
use crate::source::Source;
use crate::state::Shared;

/// What a log that is not due still needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LogToCome {
    /// `dirty over 15.2 MiB`'s figure.
    pub needs: String,
    pub has: String,
    pub coming: Vec<Span>,
}

/// The Compact now dialog: the planner's verdict and what a round would do, or why nothing can be done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CompactDialog {
    /// `61 % dirty against 50 %`, when a round is due.
    pub due: Option<String>,
    pub rewrite: Vec<Span>,
    /// Where the checkpoint is and where the round would move it.
    pub checkpoint: (i64, i64),
    /// Why nothing can be done, in a sentence.
    pub blocked: Option<String>,
    /// The Lab is switched off (`--lab false`).
    pub off: bool,
}

/// `GET /api/v1/topics/{topic}/log?partition=`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LogResponse {
    pub view: LogView,
    pub state: LogState,
    pub ruler: Ruler,
    /// The planner's rules with this partition's numbers (a log that compacts).
    pub rules: Vec<Rule>,
    pub would_rewrite: Vec<Span>,
    /// `Nov 9 21:31, 43 min ago, on inventory.levels-0.`
    pub round: Option<String>,
    pub to_come: Option<LogToCome>,
    /// Retention's rules (a log that deletes).
    pub retention_rules: Vec<Rule>,
    pub expiry: Option<Expiry>,
    pub compact: CompactDialog,
}

/// `POST /api/v1/topics/{topic}/log/compact?partition=`: what the round did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CompactDone {
    pub message: String,
}

/// `GET /api/v1/topics/{topic}/producers?partition=`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProducersResponse {
    pub view: ProducersView,
    pub held: i64,
}

/// `GET /api/v1/topics/{topic}/transactions/{id}`: the transaction, with the sentences of its two pages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TxnResponse {
    pub view: TxnView,
    pub held: i64,
    pub sentence: Vec<Span>,
    pub holds: Vec<Span>,
    /// What happens if it is not ended: before and after the refusal a fenced producer meets.
    pub unended_before: Vec<Span>,
    pub unended_after: Vec<Span>,
    /// The sentence of the page that says who is held.
    pub holding: Vec<Span>,
    pub running_ms: i64,
    pub left_ms: i64,
    pub timeout_percent: i64,
    pub timeout: String,
}

fn lab_on(s: &Shared) -> bool {
    match &s.source {
        Source::Live(l) => l.lab.is_some(),
        #[cfg(any(test, feature = "examples"))]
        Source::Fixture => false,
    }
}

fn compact_dialog(v: &LogView, lab: bool) -> CompactDialog {
    let d = crate::lab::compact::dialog(v, lab);
    CompactDialog {
        due: d.due,
        rewrite: d.rewrite,
        checkpoint: d.checkpoint,
        blocked: d.blocked,
        off: d.off,
    }
}

/// The Log answer of a partition's log.
#[must_use]
pub fn log_response(v: LogView, lab: bool) -> LogResponse {
    let state = v.state();
    let ruler = crate::log_ruler::ruler(&v);
    let (rules, would_rewrite, round, to_come) = match &v.compaction {
        Some(c) => {
            let (needs, has, coming) = crate::log_words::still_to_come(&v, c);
            (
                crate::log_words::rules(&v),
                if c.planner.due {
                    crate::log_words::would_rewrite(&v)
                } else {
                    Vec::new()
                },
                crate::log_words::round_sentence(&v, c),
                (!c.planner.due).then_some(LogToCome { needs, has, coming }),
            )
        }
        None => (Vec::new(), Vec::new(), None, None),
    };
    let (retention_rules, expiry) = if v.retention.is_some() && v.compaction.is_none() {
        (
            crate::log_words::retention_rules(&v),
            crate::log_words::expiry(&v),
        )
    } else {
        (Vec::new(), None)
    };
    let compact = compact_dialog(&v, lab);
    LogResponse {
        state,
        ruler,
        rules,
        would_rewrite,
        round,
        to_come,
        retention_rules,
        expiry,
        compact,
        view: v,
    }
}

pub(super) async fn log(
    State(s): State<Shared>,
    Path(name): Path<String>,
    RawQuery(q): RawQuery,
) -> Response {
    if !valid(&name) {
        return no_topic(&name);
    }
    let partition = partition_of(&fields(q.as_deref()));
    match s.source.log(&name, partition).await {
        Some(v) => json(StatusCode::OK, &log_response(v, lab_on(&s))),
        None => no_topic(&name),
    }
}

/// `POST /api/v1/topics/{topic}/log/compact?partition=`: the cleaner's real round, now (the Lab's Compact now). Refused,
/// with the dialog's sentence, when the topic does not compact, nothing is due or the Lab is off.
pub(super) async fn compact(
    State(s): State<Shared>,
    Path(name): Path<String>,
    RawQuery(q): RawQuery,
) -> Response {
    if !valid(&name) {
        return no_topic(&name);
    }
    let partition = partition_of(&fields(q.as_deref()));
    let live = match &s.source {
        Source::Live(l) => l,
        #[cfg(any(test, feature = "examples"))]
        Source::Fixture => {
            return error(
                StatusCode::CONFLICT,
                "LAB_OFF",
                "The Lab runs on a live broker only.",
            );
        }
    };
    let Some(lab) = &live.lab else {
        return error(
            StatusCode::CONFLICT,
            "LAB_OFF",
            "The Lab is switched off (--lab false), so the console does not run the cleaner.",
        );
    };
    match crate::gather::lab::compact(lab, &live.backend, &name, partition).await {
        Ok(()) => {
            let message = match s
                .source
                .log(&name, partition)
                .await
                .and_then(|v| v.compaction.and_then(|c| c.last))
            {
                Some(r) => format!(
                    "Ran the cleaner on {name}-{partition}: {} segment{} replaced, checkpoint at {}.",
                    r.replaced,
                    if r.replaced == 1 { "" } else { "s" },
                    crate::format::number(r.clean_to)
                ),
                None => format!("Ran the cleaner on {name}-{partition}."),
            };
            json(StatusCode::OK, &CompactDone { message })
        }
        Err(v) => error(
            StatusCode::CONFLICT,
            "COMPACT_REFUSED",
            v.refused.as_deref().unwrap_or("The round was refused."),
        ),
    }
}

pub(super) async fn producers(
    State(s): State<Shared>,
    Path(name): Path<String>,
    RawQuery(q): RawQuery,
) -> Response {
    if !valid(&name) {
        return no_topic(&name);
    }
    let partition = partition_of(&fields(q.as_deref()));
    match s.source.producers(&name, partition, Vec::new()).await {
        Some(view) => {
            let held = view.held();
            json(StatusCode::OK, &ProducersResponse { view, held })
        }
        None => no_topic(&name),
    }
}

pub(super) async fn txn(
    State(s): State<Shared>,
    Path((name, id)): Path<(String, String)>,
) -> Response {
    use crate::producers_words as w;
    if !valid(&name) {
        return no_topic(&name);
    }
    let Some(view) = s.source.txn(&name, &id).await else {
        let shown: String = id.chars().take(80).collect();
        return error(
            StatusCode::NOT_FOUND,
            "TRANSACTIONAL_ID_NOT_FOUND",
            &format!(
                "The broker knows no transaction called {shown}, or the console may not describe it. A transactional id is forgotten after 7 days idle."
            ),
        );
    };
    let (unended_before, unended_after) = w::unended_parts(&view);
    let x = &view.txn;
    json(
        StatusCode::OK,
        &TxnResponse {
            held: view.held(),
            sentence: w::txn_sentence(&view),
            holds: w::holds_sentence(&view),
            unended_before,
            unended_after,
            holding: w::holding_sentence(&view),
            running_ms: w::running_ms(x, view.now_ms),
            left_ms: w::left_ms(x, view.now_ms),
            timeout_percent: w::timeout_percent(x, view.now_ms),
            timeout: w::timeout_text(x.timeout_ms),
            view,
        },
    )
}
