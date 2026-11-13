//! The Config tab of a topic in the React console (slice R1b): every config with the words Rust decides for it, the edit
//! dialog (one value, a list, the two compaction lags), Check (`validate_only`) and Save, and Reset. The same gathers as
//! the classic Config pages (`gather/config*.rs`), the same IncrementalAlterConfigs, the same refusal words.
//!
//! The "Where from" places and the group ledes are said here rather than borrowed from `pages/config`, which slice R5
//! removes with the rest of the page layer.

use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::get;
use kafka_wire::ids::TopicName;
use serde::Serialize;

use super::json;
use super::topic_writes::no_topic;
use crate::config_effects::{page_moment, sentence_of};
use crate::config_words::{
    Group, first_sentence, group_of, has_more, page_sentence, plain_text, row_gloss, type_word,
};
use crate::model::{Cleanup, ConfigAsk, ConfigRow, ConfigSource, ConfigSynonym, Span};
use crate::state::Shared;

pub(super) fn valid(topic: &str) -> bool {
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

/// The routes of the Config tab under `web`.
pub(super) fn routes(web: &crate::base::Base) -> Router<Shared> {
    Router::new().route(&web.url("/api/v1/topics/{topic}/config"), get(tab))
}
