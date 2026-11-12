//! A row of the Config table and the two things that open under it: where its value comes from, and Kafka's text.

use maud::{Markup, html};

use super::common::{edit_url, plain_value, reset_url, source_tag, value_cell};
use super::links::token;
use crate::base::Base;
use crate::config_effects::sentence_of;
use crate::config_words::{first_sentence, has_more, plain_text, type_word};
use crate::model::{ConfigAsk, ConfigPace, ConfigRow, ConfigSource, ConfigSynonym};
use crate::ui::button::{Size, Variant, class};
use crate::ui::client::dialog_link;
use crate::ui::components::{Menu, MenuItem, menu};
use crate::ui::disclosure::{Look, disclosure};
use crate::ui::glyph::{Glyph, glyph};

/// What a row needs besides itself.
#[derive(Clone, Copy)]
pub(super) struct Ctx<'a> {
    pub base: &'a Base,
    pub topic: &'a str,
    pub ask: &'a ConfigAsk,
    pub pace: Option<ConfigPace>,
}

fn opened(c: &Ctx<'_>, kind: &str, key: &str) -> bool {
    c.ask.open.contains(&token(kind, key))
}

/// One of the two panels under a row: a native disclosure that opens in place (no navigation, the scroll stays), shut unless
/// the address asked for it (`?open=where:segment.bytes`, a deep link).
fn panel(c: &Ctx<'_>, kind: &str, r: &ConfigRow, label: &str, body: Markup) -> Markup {
    disclosure(
        Look::Button,
        Some(&format!("{kind}-{}", r.name)),
        opened(c, kind, &r.name),
        html! { (label) },
        body,
    )
}

fn first_cell(r: &ConfigRow, c: &Ctx<'_>) -> Markup {
    html! {
        td {
            span class="mono bright" { (r.name) }
            @if !r.doc.is_empty() { span class="sub" { (first_sentence(&r.doc)) } }
            @if let Some(s) = sentence_of(&r.name, c.pace) { span class="sub" { (s) } }
            @if r.changed() || has_more(&r.doc) {
                div class="panels" {
                    @if r.changed() { (panel(c, "where", r, "Where from", where_from(r))) }
                    @if has_more(&r.doc) { (panel(c, "text", r, "Kafka's text", kafkas_text(r))) }
                }
            }
        }
    }
}

fn source_cell(r: &ConfigRow) -> Markup {
    html! { td { (source_tag(r.source)) } }
}

/// A topic value has two actions (Edit, Reset): a menu. A value that is the broker's or Kafka's has one: a button.
fn actions_cell(r: &ConfigRow, c: &Ctx<'_>) -> Markup {
    let edit = edit_url(c.base, c.topic, &r.name);
    let reset = reset_url(c.base, c.topic, &r.name);
    let items = [
        MenuItem::Link {
            label: "Edit",
            href: &edit,
            dialog: true,
            danger: false,
        },
        MenuItem::Link {
            label: "Reset to the inherited value",
            href: &reset,
            dialog: true,
            danger: false,
        },
    ];
    let label = format!("Actions for {}", r.name);
    html! {
        td {
            div class="row" style="justify-content:flex-end;gap:6px" {
                @if r.on_topic() {
                    (menu(&Menu { id: &format!("cfg-{}", r.name), label: &label, items: &items }))
                } @else {
                    (dialog_link(&class(Variant::Plain, Size::Small), &edit, html! { "Edit" }))
                }
            }
        }
    }
}

/// The row, and under it what has been opened.
pub(super) fn row(r: &ConfigRow, c: &Ctx<'_>) -> Markup {
    html! {
        tr id=(format!("c-{}", r.name)) {
            (first_cell(r, c)) (value_cell(r)) (source_cell(r)) (actions_cell(r, c))
        }
    }
}

/// The places a value can come from, in the order the broker looks, with the one that applies marked.
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

fn place_row(r: &ConfigRow, place: &(ConfigSource, &str, &str), wins: bool) -> Markup {
    let (source, title, detail) = *place;
    let syn: Option<&ConfigSynonym> = r.synonyms.iter().find(|s| s.source == source);
    let behind = r
        .synonyms
        .iter()
        .find(|s| s.source != ConfigSource::DynamicTopic)
        .map(|s| s.name.as_str());
    let name = syn.map(|s| s.name.as_str()).or(behind).unwrap_or(&r.name);
    html! {
        div class={"r " @if wins { "ok" } @else { "no" }} style="text-align:left" {
            (glyph(if wins { Glyph::Check } else { Glyph::X }, 16, ""))
            div {
                (title)
                span class="sub" {
                    @if source == ConfigSource::DynamicTopic { (r.name) " " (detail) }
                    @else if syn.is_some() { (name) ": " (detail) }
                    @else { (name) ": not set" }
                }
            }
            span class="v" { (syn.map_or("none", |s| s.value.as_str())) }
        }
    }
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

fn where_from(r: &ConfigRow) -> Markup {
    let winner = PLACES
        .iter()
        .position(|p| r.synonyms.iter().any(|s| s.source == p.0))
        .unwrap_or(PLACES.len());
    html! {
        div class="panel" {
            b { "Where " (r.name) " comes from." }
            " The first place with a value wins. DescribeConfigs lists them when synonyms are asked for."
            div class="rules" style="margin-top:12px" {
                @for (i, p) in PLACES.iter().enumerate() { (place_row(r, p, i == winner)) }
            }
            @if let Some(t) = takes_over(r) { p class="help" style="margin:12px 0 0" { (t) } }
        }
    }
}

fn kafkas_text(r: &ConfigRow) -> Markup {
    let default = r.kafka_default().map_or("none", |d| d.value.as_str());
    html! {
        div class="panel" {
            b { "Kafka's text for " (r.name) }
            ", as DescribeConfigs returns it when documentation is asked for."
            div class="teach" style="max-width:none;margin-top:12px" {
                div style="font-size:13px;line-height:21px" { (plain_text(&r.doc)) }
            }
            dl class="kv sm" style="margin-top:12px" {
                dt { "Type" } dd class="mono" { (type_word(r.kind)) }
                dt { "Allowed" } dd { (r.allowed) }
                dt { "Default" } dd class="mono" { (default) }
                dt { "Here" } dd { (plain_value(&r.name, &r.value)) span class="sub" { (r.source.label()) } }
            }
        }
    }
}
