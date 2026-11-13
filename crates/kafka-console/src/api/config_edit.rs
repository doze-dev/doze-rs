//! The edit and reset dialogs of the Config tab (slice R1b): Check (`validate_only`), Save and Reset, answered as `topic_writes` says.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::get;
use serde::Serialize;

use super::config::valid;
use super::topic_writes::{invalid, kafka_refused, no_topic};
use super::{error, json};
use crate::config_effects::{moment, reader, sees, sentence, takes_effect, when_label};
use crate::config_teach::MAX_LAG;
use crate::config_words::gloss;
use crate::form::{get as field, parse, parse_body, text};
use crate::gather::{ConfigFields, ConfigMissing};
use crate::model::{
    ConfigEditView, ConfigOp, ConfigRefusal, ConfigRefused, ConfigResetResult, ConfigResetView,
    ConfigResult, ConfigSaved, Span,
};
use crate::state::Shared;

// ---- Edit ---------------------------------------------------------------------------------------------------------

/// `GET /api/v1/topics/{topic}/config/edit?key=&op=&value=&other=`: the dialog for what was typed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfigEditForm {
    pub view: ConfigEditView,
    pub title: String,
    /// The label of the value field: `New value for orders`, `Policy to append`.
    pub label: String,
    /// What the typed values come to in words (`7 days`), for the value and the second lag.
    pub unit: Option<String>,
    pub other_unit: Option<String>,
    /// A list's entries now.
    pub have: Vec<String>,
}

/// A save the broker took (200), with the board's words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfigSavedAnswer {
    pub saved: ConfigSaved,
    pub sentence: Vec<Span>,
    /// What it does to this log, when the broker reads it, and what reads it.
    pub effect: Option<String>,
    pub when: Option<String>,
    pub reader: Option<String>,
}

/// What Save or Check came to (200).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConfigEditAnswer {
    /// Kafka would take it (`validate_only`); nothing was changed.
    Checked {
        form: Box<ConfigEditForm>,
    },
    Saved {
        answer: Box<ConfigSavedAnswer>,
    },
}

/// A refused save (the `detail` of a 422).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfigRefusedAnswer {
    pub refused: ConfigRefused,
    pub sentence: String,
    /// The values to fill the dialog again with: `(field, value)`, field `value` or `other`.
    pub again: Vec<(String, String)>,
}

fn label_of(v: &ConfigEditView) -> String {
    match v.form {
        crate::model::ConfigForm::List => {
            let what = if v.row.name == "cleanup.policy" {
                "Policy"
            } else {
                "Value"
            };
            let to = match v.input.op {
                ConfigOp::Subtract => "to take out",
                ConfigOp::Set => "to use",
                _ => "to append",
            };
            format!("{what} {to}")
        }
        crate::model::ConfigForm::Lags => v.row.name.clone(),
        crate::model::ConfigForm::Single => format!("New value for {}", v.topic),
    }
}

fn edit_form_of(v: ConfigEditView) -> ConfigEditForm {
    let max = v
        .other
        .as_ref()
        .map_or(MAX_LAG, |o| o.name.as_str())
        .to_owned();
    ConfigEditForm {
        title: match v.form {
            crate::model::ConfigForm::Lags => "Edit compaction lag".to_owned(),
            _ => format!("Edit {}", v.row.name),
        },
        label: label_of(&v),
        unit: gloss(&v.row.name, &v.input.value),
        other_unit: gloss(&max, &v.input.other),
        have: v
            .row
            .value
            .split(',')
            .map(str::trim)
            .filter(|e| !e.is_empty())
            .map(str::to_owned)
            .collect(),
        view: v,
    }
}

fn missing(topic: &str, key: &str, why: ConfigMissing) -> Response {
    match why {
        ConfigMissing::Topic => no_topic(topic),
        ConfigMissing::Key => error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            &format!(
                "The topic {topic} has no config called {}.",
                key.chars().take(80).collect::<String>()
            ),
        ),
    }
}

async fn edit_form(
    State(s): State<Shared>,
    Path(topic): Path<String>,
    RawQuery(q): RawQuery,
) -> Response {
    let f = parse(q.as_deref().unwrap_or(""));
    let key = field(&f, "key").unwrap_or("");
    if !valid(&topic) {
        return no_topic(&topic);
    }
    let fields = ConfigFields {
        key,
        op: field(&f, "op"),
        value: field(&f, "value"),
        other: field(&f, "other"),
    };
    match s.source.config_form(&topic, fields).await {
        Ok(v) => json(StatusCode::OK, &edit_form_of(v)),
        Err(why) => missing(&topic, key, why),
    }
}

fn saved_answer(saved: ConfigSaved) -> ConfigSavedAnswer {
    let first = saved
        .changes
        .first()
        .map(|c| c.name.clone())
        .unwrap_or_default();
    let m = moment(&first);
    let tail = m.map_or_else(String::new, |m| sees(m, saved.pace));
    let others = saved.total.saturating_sub(saved.changes.len());
    let mut out = Vec::new();
    for (i, c) in saved.changes.iter().enumerate() {
        if i > 0 {
            out.push(Span::text(" and "));
        }
        out.push(Span::strong(c.name.clone()));
    }
    out.push(Span::text(if saved.changes.len() == 1 {
        " is now "
    } else {
        " are now "
    }));
    for (i, c) in saved.changes.iter().enumerate() {
        if i > 0 {
            out.push(Span::text(" and "));
        }
        out.push(Span::strong(
            c.now.as_ref().map_or("not set", |n| n.value.as_str()),
        ));
    }
    out.push(Span::text(format!(
        ", set on this topic. The other {} configs are unchanged; {tail}.",
        crate::format::number(i64::try_from(others).unwrap_or(0))
    )));
    ConfigSavedAnswer {
        effect: m.map(|m| sentence(m, saved.pace)),
        when: m.map(|m| when_label(m, saved.pace)),
        reader: m.map(|m| reader(m).to_owned()),
        sentence: out,
        saved,
    }
}

fn refused_answer(r: ConfigRefused) -> ConfigRefusedAnswer {
    let said = match (r.kind, r.refusal.code) {
        (ConfigRefusal::Lags, _) => {
            "Kafka checks the two lags together and found the minimum above the maximum.".to_owned()
        }
        (_, 40) => format!("Kafka refused the value you sent for {}.", r.input.key),
        _ => "Kafka refused the request.".to_owned(),
    };
    let pairs: Vec<(String, String)> = match (r.kind, r.pass.first()) {
        (_, Some(p)) => p.pairs.clone(),
        (ConfigRefusal::Lags, None) => vec![
            (
                crate::config_teach::MIN_LAG.to_owned(),
                r.input.value.clone(),
            ),
            (MAX_LAG.to_owned(), r.input.other.clone()),
        ],
        (ConfigRefusal::Value, None) => Vec::new(),
    };
    ConfigRefusedAnswer {
        sentence: format!("Nothing was changed. {said}"),
        again: pairs
            .into_iter()
            .map(|(k, v)| {
                let name = if k == MAX_LAG { "other" } else { "value" };
                (name.to_owned(), v)
            })
            .collect(),
        refused: r,
    }
}

/// `POST /api/v1/topics/{topic}/config/edit` (`check=1`: `validate_only`, nothing changes).
async fn edit(State(s): State<Shared>, Path(topic): Path<String>, body: Bytes) -> Response {
    let f = parse_body(&body);
    let key = text(&f, "key");
    if !valid(&topic) {
        return no_topic(&topic);
    }
    let fields = ConfigFields {
        key,
        op: Some(text(&f, "op")),
        value: Some(text(&f, "value")),
        other: Some(text(&f, "other")),
    };
    let check = field(&f, "check") == Some("1");
    match s.source.config_save(&topic, fields, check).await {
        ConfigResult::Saved(v) => json(
            StatusCode::OK,
            &ConfigEditAnswer::Saved {
                answer: Box::new(saved_answer(*v)),
            },
        ),
        ConfigResult::Checked(v) => json(
            StatusCode::OK,
            &ConfigEditAnswer::Checked {
                form: Box::new(edit_form_of(*v)),
            },
        ),
        ConfigResult::Refused(r) => {
            let refusal = r.refusal.clone();
            kafka_refused(&refusal, &refused_answer(*r))
        }
        ConfigResult::Invalid(v) => {
            let first = v.error.clone().unwrap_or_default();
            invalid(&first, &edit_form_of(*v))
        }
        ConfigResult::NoTopic => no_topic(&topic),
        ConfigResult::NoKey => missing(&topic, key, ConfigMissing::Key),
    }
}

// ---- Reset --------------------------------------------------------------------------------------------------------

/// `GET /api/v1/topics/{topic}/config/reset?key=`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfigResetForm {
    pub view: ConfigResetView,
    pub now_gloss: Option<String>,
    pub after_gloss: Option<String>,
    pub takes_effect: Option<String>,
}

/// A reset the broker took (200).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfigResetDone {
    pub key: String,
}

fn reset_form_of(v: ConfigResetView) -> ConfigResetForm {
    ConfigResetForm {
        now_gloss: gloss(&v.row.name, &v.row.value),
        after_gloss: v.after.as_ref().and_then(|a| gloss(&v.row.name, &a.value)),
        takes_effect: moment(&v.row.name).map(|m| takes_effect(m, v.pace)),
        view: v,
    }
}

async fn reset_form(
    State(s): State<Shared>,
    Path(topic): Path<String>,
    RawQuery(q): RawQuery,
) -> Response {
    let f = parse(q.as_deref().unwrap_or(""));
    let key = field(&f, "key").unwrap_or("");
    if !valid(&topic) {
        return no_topic(&topic);
    }
    match s.source.config_reset_form(&topic, key).await {
        Ok(v) => json(StatusCode::OK, &reset_form_of(v)),
        Err(why) => missing(&topic, key, why),
    }
}

async fn reset(State(s): State<Shared>, Path(topic): Path<String>, body: Bytes) -> Response {
    let f = parse_body(&body);
    let key = text(&f, "key");
    if !valid(&topic) {
        return no_topic(&topic);
    }
    match s.source.config_reset(&topic, key).await {
        ConfigResetResult::Done => json(
            StatusCode::OK,
            &ConfigResetDone {
                key: key.to_owned(),
            },
        ),
        ConfigResetResult::Refused(v) => match v.refusal.clone() {
            Some(r) => kafka_refused(&r, &reset_form_of(*v)),
            None => invalid("Refused.", &reset_form_of(*v)),
        },
        ConfigResetResult::NoTopic => no_topic(&topic),
        ConfigResetResult::NoKey => missing(&topic, key, ConfigMissing::Key),
    }
}

/// The routes of this file under `web`.
pub(super) fn routes(web: &crate::base::Base) -> Router<Shared> {
    Router::new()
        .route(
            &web.url("/api/v1/topics/{topic}/config/edit"),
            get(edit_form).post(edit),
        )
        .route(
            &web.url("/api/v1/topics/{topic}/config/reset"),
            get(reset_form).post(reset),
        )
}
