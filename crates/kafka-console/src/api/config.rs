//! The Config tab of a topic in the React console (slice R1b): every config with the words Rust decides for it, the edit
//! dialog (one value, a list, the two compaction lags), Check (`validate_only`) and Save, and Reset. The same gathers as
//! the classic Config pages (`gather/config*.rs`), the same IncrementalAlterConfigs, the same refusal words.
//!
//! The "Where from" places and the group ledes are said here rather than borrowed from `pages/config`, which slice R5
//! removes with the rest of the page layer.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::get;
use kafka_wire::ids::TopicName;
use serde::Serialize;

use super::topic_writes::{invalid, kafka_refused, no_topic};
use super::{error, json};
use crate::config_effects::{moment, page_moment, reader, sees, sentence, sentence_of, takes_effect, when_label};
use crate::config_teach::MAX_LAG;
use crate::config_words::{
    Group, first_sentence, gloss, group_of, has_more, page_sentence, plain_text, row_gloss,
    type_word,
};
use crate::form::{get as field, parse, parse_body, text};
use crate::gather::{ConfigFields, ConfigMissing};
use crate::model::{
    Cleanup, ConfigAsk, ConfigEditView, ConfigOp, ConfigRefusal, ConfigRefused, ConfigResetResult,
    ConfigResetView, ConfigResult, ConfigRow, ConfigSaved, ConfigSource, ConfigSynonym, Span,
};
use crate::state::Shared;

fn valid(topic: &str) -> bool {
    TopicName::parse(topic).is_ok()
}

/// One place a value could come from, as "Where from" lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfigPlace {
    pub title: String,
    /// `log.segment.bytes: not set`.
    pub detail: String,
    /// The value there, `none` when it has none.
    pub value: String,
    /// The first place with a value: the one that applies.
    pub wins: bool,
}

/// A row of the Config tab: the config and every word the page says of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfigRowView {
    pub name: String,
    pub value: String,
    pub source: ConfigSource,
    /// `Dynamic topic`, `Default`.
    pub source_label: String,
    /// The small line under the value: `7 days`, `about 1 MiB`.
    pub gloss: Option<String>,
    /// The group it is listed in (`cleanup`, `retention`, ...).
    pub group: String,
    /// Not Kafka's default: set on the topic or by a flag.
    pub changed: bool,
    /// The topic sets it: it can be reset.
    pub on_topic: bool,
    /// Kafka's first sentence, and whether its text says more.
    pub doc_first: String,
    pub doc_more: bool,
    /// Kafka's whole text, plain.
    pub doc: String,
    /// When a change counts.
    pub effect: Option<String>,
    pub type_word: String,
    pub allowed: String,
    /// Kafka's default, where the broker named it.
    pub default: Option<String>,
    /// Every place the value could come from (for a changed row).
    pub places: Vec<ConfigPlace>,
    /// `Reset the topic value and Kafka's default, 604800000, takes over.`
    pub takes_over: Option<String>,
}

/// A group of rows on the tab, in the order the page shows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfigGroup {
    pub id: String,
    pub title: String,
    pub lede: String,
}

/// `GET /api/v1/topics/{topic}/config`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfigTab {
    pub topic: String,
    pub cleanup: Cleanup,
    /// The sentence under the tabs (static: counts, no live figure).
    pub sentence: Vec<Span>,
    pub groups: Vec<ConfigGroup>,
    pub rows: Vec<ConfigRowView>,
}

const PLACES: [(ConfigSource, &str, &str); 5] = [
    (ConfigSource::DynamicTopic, "Dynamic topic", "on this topic"),
    (
        ConfigSource::DynamicBroker,
        "Dynamic broker, this broker",
        "set for this broker",
    ),
    (
        ConfigSource::DynamicDefault,
        "Dynamic broker, all brokers",
        "set for all brokers",
    ),
    (
        ConfigSource::StaticFlag,
        "Static flag",
        "a start flag that is not Kafka's default",
    ),
    (ConfigSource::Default, "Kafka's default", "Kafka's own"),
];

fn places(r: &ConfigRow) -> Vec<ConfigPlace> {
    let winner = PLACES
        .iter()
        .position(|p| r.synonyms.iter().any(|s| s.source == p.0))
        .unwrap_or(PLACES.len());
    let behind = r
        .synonyms
        .iter()
        .find(|s| s.source != ConfigSource::DynamicTopic)
        .map(|s| s.name.as_str());
    PLACES
        .iter()
        .enumerate()
        .map(|(i, (source, title, detail))| {
            let syn: Option<&ConfigSynonym> = r.synonyms.iter().find(|s| s.source == *source);
            let name = syn.map(|s| s.name.as_str()).or(behind).unwrap_or(&r.name);
            ConfigPlace {
                title: (*title).to_owned(),
                detail: if *source == ConfigSource::DynamicTopic {
                    format!("{} {detail}", r.name)
                } else if syn.is_some() {
                    format!("{name}: {detail}")
                } else {
                    format!("{name}: not set")
                },
                value: syn.map_or("none", |s| s.value.as_str()).to_owned(),
                wins: i == winner,
            }
        })
        .collect()
}

fn takes_over(r: &ConfigRow) -> Option<String> {
    let next = r.fallback().filter(|_| r.on_topic())?;
    let who = match next.source {
        ConfigSource::StaticFlag => "the flag",
        ConfigSource::Default => "Kafka's default",
        _ => "the broker's value",
    };
    Some(format!(
        "Reset the topic value and {who}, {}, takes over.",
        next.value
    ))
}

const fn group_id(g: Group) -> &'static str {
    match g {
        Group::Cleanup => "cleanup",
        Group::Retention => "retention",
        Group::Segments => "segments",
        Group::Records => "records",
        Group::Durability => "durability",
        Group::Replication => "replication",
    }
}

fn lede_of(g: Group, cleanup: Cleanup) -> &'static str {
    match g {
        Group::Cleanup => "How the cleaner decides.",
        Group::Retention if cleanup == Cleanup::Compact => {
            "Not used by this topic: it only compacts."
        }
        Group::Retention => "When old records are deleted by age or size.",
        Group::Replication => {
            "Configs Kafka lists that this broker has no use for: one broker, no replicas, no tiered storage."
        }
        _ => "",
    }
}

fn row_view(r: &ConfigRow, pace: Option<crate::model::ConfigPace>) -> ConfigRowView {
    ConfigRowView {
        name: r.name.clone(),
        value: r.value.clone(),
        source: r.source,
        source_label: r.source.label().to_owned(),
        gloss: row_gloss(r),
        group: group_id(group_of(&r.name)).to_owned(),
        changed: r.changed(),
        on_topic: r.on_topic(),
        doc_first: first_sentence(&r.doc),
        doc_more: has_more(&r.doc),
        doc: plain_text(&r.doc),
        effect: sentence_of(&r.name, pace),
        type_word: type_word(r.kind).to_owned(),
        allowed: r.allowed.clone(),
        default: r.kafka_default().map(|d| d.value.clone()),
        places: if r.changed() { places(r) } else { Vec::new() },
        takes_over: takes_over(r),
    }
}

async fn tab(State(s): State<Shared>, Path(topic): Path<String>) -> Response {
    if !valid(&topic) {
        return no_topic(&topic);
    }
    let Some(p) = s.source.config_page(&topic, ConfigAsk::default()).await else {
        return no_topic(&topic);
    };
    json(
        StatusCode::OK,
        &ConfigTab {
            sentence: page_sentence(&p.rows, &page_moment(p.pace)),
            groups: Group::ALL
                .iter()
                .map(|g| ConfigGroup {
                    id: group_id(*g).to_owned(),
                    title: g.title().to_owned(),
                    lede: lede_of(*g, p.head.cleanup).to_owned(),
                })
                .collect(),
            rows: p.rows.iter().map(|r| row_view(r, p.pace)).collect(),
            cleanup: p.head.cleanup,
            topic: p.head.name,
        },
    )
}

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
    Checked { form: Box<ConfigEditForm> },
    Saved { answer: Box<ConfigSavedAnswer> },
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
            (crate::config_teach::MIN_LAG.to_owned(), r.input.value.clone()),
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

/// The routes of the Config tab under `web`.
pub(super) fn routes(web: &crate::base::Base) -> Router<Shared> {
    Router::new()
        .route(&web.url("/api/v1/topics/{topic}/config"), get(tab))
        .route(
            &web.url("/api/v1/topics/{topic}/config/edit"),
            get(edit_form).post(edit),
        )
        .route(
            &web.url("/api/v1/topics/{topic}/config/reset"),
            get(reset_form).post(reset),
        )
}
